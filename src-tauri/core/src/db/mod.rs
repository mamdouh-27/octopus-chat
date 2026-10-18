use rusqlite::Connection;

use crate::error::Result;

mod schema;
mod store;

pub use schema::MIGRATIONS;
pub use store::{ProjectStats, Store};

/// Opens the database, applies pragmas tuned for a desktop app and runs
/// migrations. The connection is reused for the lifetime of the process.
pub fn open(path: &std::path::Path) -> Result<Connection> {
    let conn = Connection::open(path)?;
    configure(&conn)?;
    migrate(&conn)?;
    Ok(conn)
}

fn configure(conn: &Connection) -> Result<()> {
    // WAL keeps reads from blocking the agent's writes.
    conn.pragma_update(None, "journal_mode", "WAL")?;
    // NORMAL is the standard durability/throughput trade-off under WAL.
    conn.pragma_update(None, "synchronous", "NORMAL")?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
    conn.pragma_update(None, "temp_store", "MEMORY")?;
    conn.busy_timeout(std::time::Duration::from_secs(10))?;
    Ok(())
}

fn migrate(conn: &Connection) -> Result<()> {
    let current: i64 =
        conn.query_row("PRAGMA user_version", [], |r| r.get(0)).unwrap_or(0);
    for (version, sql) in MIGRATIONS.iter().enumerate() {
        let target = (version + 1) as i64;
        if target <= current {
            continue;
        }
        conn.execute_batch(sql)?;
        // user_version is a plain integer; PRAGMA does not accept a bound param.
        conn.pragma_update(None, "user_version", target)?;
        if target == 3 {
            repair_project_roots(conn)?;
        }
    }
    Ok(())
}

/// Rewrites project roots that were stored in the Windows extended-length form.
///
/// Versions before v3 saved roots exactly as `Path::canonicalize` returned
/// them, which on Windows carries a `\\?\` prefix. That form fails the
/// sandbox's own containment check, since tool arguments resolve to ordinary
/// `C:\...` paths, so an affected project denied every read and write.
///
/// The value is decoded and re-encoded rather than string-replaced: `roots` is
/// a JSON array, and JSON escapes every backslash, so the four characters
/// `\\?\` also occur at a misaligned offset inside the encoded text where a
/// textual replacement would corrupt the path.
fn repair_project_roots(conn: &Connection) -> Result<()> {
    let rows: Vec<(String, String)> = {
        let mut stmt = conn.prepare("SELECT id, roots FROM projects")?;
        let mapped = stmt.query_map([], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
        })?;
        mapped.collect::<std::result::Result<Vec<_>, _>>()?
    };

    for (id, raw) in rows {
        let Ok(roots) = serde_json::from_str::<Vec<String>>(&raw) else {
            continue;
        };
        let fixed: Vec<String> = roots
            .iter()
            .map(|r| crate::paths::strip_extended_prefix(r))
            .collect();
        if fixed == roots {
            continue;
        }
        conn.execute(
            "UPDATE projects SET roots = ?2 WHERE id = ?1",
            rusqlite::params![id, serde_json::to_string(&fixed)?],
        )?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A database already on the version before the repair, holding one project.
    fn pre_v3_db(roots_json: &str) -> Connection {
        let conn = Connection::open_in_memory().expect("in-memory db");
        for sql in MIGRATIONS.iter().take(2) {
            conn.execute_batch(sql).expect("v1/v2 should apply");
        }
        conn.pragma_update(None, "user_version", 2i64).unwrap();
        conn.execute(
            "INSERT INTO projects (id,name,roots,created_at,updated_at)
             VALUES ('p1','demo',?1,1,1)",
            [roots_json],
        )
        .expect("seed project");
        conn
    }

    fn roots_of(conn: &Connection) -> Vec<String> {
        let raw: String = conn
            .query_row("SELECT roots FROM projects WHERE id='p1'", [], |r| r.get(0))
            .expect("project row");
        serde_json::from_str(&raw).expect("roots should be valid json")
    }

    #[test]
    fn repair_rewrites_drive_roots() {
        let conn = pre_v3_db(r#"["\\\\?\\C:\\work\\app"]"#);
        repair_project_roots(&conn).unwrap();
        assert_eq!(roots_of(&conn), vec![r"C:\work\app".to_string()]);
    }

    #[test]
    fn repair_rewrites_unc_roots_without_a_stray_prefix() {
        let conn = pre_v3_db(r#"["\\\\?\\UNC\\server\\share\\proj"]"#);
        repair_project_roots(&conn).unwrap();
        assert_eq!(roots_of(&conn), vec![r"\\server\share\proj".to_string()]);
    }

    #[test]
    fn repair_handles_a_mixed_multi_root_project() {
        let conn = pre_v3_db(r#"["\\\\?\\C:\\a","D:\\b","\\\\?\\UNC\\s\\c"]"#);
        repair_project_roots(&conn).unwrap();
        assert_eq!(
            roots_of(&conn),
            vec![r"C:\a".to_string(), r"D:\b".to_string(), r"\\s\c".to_string()]
        );
    }

    #[test]
    fn repair_leaves_healthy_roots_alone() {
        let before = vec![r"D:\work\app".to_string(), r"D:\work\lib".to_string()];
        let conn = pre_v3_db(&serde_json::to_string(&before).unwrap());
        repair_project_roots(&conn).unwrap();
        assert_eq!(roots_of(&conn), before);
    }

    #[test]
    fn repair_is_idempotent() {
        let conn = pre_v3_db(r#"["\\\\?\\C:\\work\\app"]"#);
        repair_project_roots(&conn).unwrap();
        let once = roots_of(&conn);
        repair_project_roots(&conn).unwrap();
        assert_eq!(roots_of(&conn), once);
    }

    #[test]
    fn repair_skips_a_row_whose_roots_are_not_json() {
        let conn = pre_v3_db("not json at all");
        repair_project_roots(&conn).unwrap();
        let raw: String = conn
            .query_row("SELECT roots FROM projects WHERE id='p1'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(raw, "not json at all");
    }

    #[test]
    fn opening_a_v2_database_runs_the_repair() {
        let dir = tempdir::TempDir::new("octopus-migrate").expect("tempdir");
        let path = dir.path().join("db.sqlite");
        {
            let conn = Connection::open(&path).unwrap();
            for sql in MIGRATIONS.iter().take(2) {
                conn.execute_batch(sql).unwrap();
            }
            conn.pragma_update(None, "user_version", 2i64).unwrap();
            conn.execute(
                "INSERT INTO projects (id,name,roots,created_at,updated_at)
                 VALUES ('p1','demo',?1,1,1)",
                [r#"["\\\\?\\C:\\work\\app"]"#],
            )
            .unwrap();
        }
        // Reopening applies v3, which must heal the seeded root.
        let conn = open(&path).expect("open should apply migrations");
        let raw: String = conn
            .query_row("SELECT roots FROM projects WHERE id='p1'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(raw, r#"["C:\\work\\app"]"#);
    }

    #[test]
    fn v4_lets_a_model_reference_a_provider_with_no_custom_row() {
        let dir = tempdir::TempDir::new("octopus-v4").expect("tempdir");
        let path = dir.path().join("db.sqlite");
        let conn = open(&path).expect("open applies every migration");

        // `ollama` is a catalog provider: it has no row in custom_providers, and
        // the old foreign key made this insert fail outright.
        conn.execute(
            "INSERT INTO custom_models (id,provider_id,label,dialect,position)
             VALUES ('llama3','ollama','Llama 3','openai',0)",
            [],
        )
        .expect("a model may belong to a built-in provider");

        // The same model id under a second provider is legitimate and must not
        // collide now that the key is (provider_id, id).
        conn.execute(
            "INSERT INTO custom_models (id,provider_id,label,dialect,position)
             VALUES ('llama3','lmstudio','Llama 3','openai',0)",
            [],
        )
        .expect("the same id under another provider is a distinct row");

        let n: i64 = conn
            .query_row("SELECT count(*) FROM custom_models WHERE id='llama3'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 2);
    }

    #[test]
    fn v4_preserves_existing_models() {
        let dir = tempdir::TempDir::new("octopus-v4b").expect("tempdir");
        let path = dir.path().join("db.sqlite");
        {
            let conn = Connection::open(&path).unwrap();
            for sql in MIGRATIONS.iter().take(3) {
                conn.execute_batch(sql).unwrap();
            }
            conn.pragma_update(None, "user_version", 3i64).unwrap();
            conn.execute(
                "INSERT INTO custom_providers (id,label,base_url) VALUES ('e','E','http://x/v1')",
                [],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO custom_models (id,provider_id,label,dialect,position)
                 VALUES ('kept','e','Kept','openai',3)",
                [],
            )
            .unwrap();
        }
        let conn = open(&path).expect("open should apply v4");
        let (label, pos): (String, i64) = conn
            .query_row("SELECT label,position FROM custom_models WHERE id='kept'", [], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .expect("the pre-existing row must survive the rebuild");
        assert_eq!(label, "Kept");
        assert_eq!(pos, 3);
    }}
