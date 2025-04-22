use std::sync::Arc;

use app_lib::domain::traits::{MockMediaChecker, Checker};

pub fn get_checker() -> Checker {
    let mock_checker = MockMediaChecker::new();

    Arc::new(mock_checker)
}
