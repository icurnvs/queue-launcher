use crate::Result;
use std::panic::{catch_unwind, AssertUnwindSafe};

pub fn exit_code(check: impl FnOnce() -> Result<()>) -> i32 {
    match catch_unwind(AssertUnwindSafe(check)) {
        Ok(Ok(())) => 0,
        Ok(Err(error)) => {
            eprintln!("FAIL: {error}");
            1
        }
        Err(_) => {
            eprintln!("FAIL: offline verification panicked");
            1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::exit_code;

    #[test]
    fn successful_verification_exits_successfully() {
        assert_eq!(exit_code(|| Ok(())), 0);
    }

    #[test]
    fn verification_error_exits_with_failure() {
        assert_eq!(exit_code(|| Err("verification failed".into())), 1);
    }

    #[test]
    fn verification_panic_exits_with_failure() {
        assert_eq!(exit_code(|| panic!("verification panicked")), 1);
    }
}
