use crate::core::db::repository::Database;

impl Database {
    #[cfg(test)]
    pub fn count_columns(&self, table: &str) -> i64 {
        self.conn
            .query_row(&format!("SELECT COUNT(*) FROM pragma_table_info('{table}')"), [], |r| {
                r.get(0)
            })
            .unwrap_or(-1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn intent_tables_exist_with_expected_columns() {
        let db = Database::new_test().unwrap();
        let domains = db.count_columns("intent_domains");
        let intents = db.count_columns("intents");
        assert_eq!(domains, 8, "intent_domains 列数不符");
        assert_eq!(intents, 21, "intents 列数不符");
    }
}
