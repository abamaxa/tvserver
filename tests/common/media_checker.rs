use std::sync::Arc;

use tvserver::domain::traits::{MockMediaChecker, Checker};

pub fn get_checker() -> Checker {
    let mock_checker = MockMediaChecker::new();

    Arc::new(mock_checker)
}
