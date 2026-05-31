#![allow(
    clippy::cast_precision_loss,
    clippy::let_and_return,
    clippy::manual_let_else,
    clippy::missing_const_for_fn,
    clippy::must_use_candidate,
    clippy::needless_pass_by_value,
    clippy::non_std_lazy_statics,
    clippy::single_match_else,
    clippy::struct_field_names,
    clippy::suboptimal_flops,
    clippy::unchecked_time_subtraction,
    clippy::unused_async
)]

use std::time::Instant;

pub struct TokenBucket {
    tokens: f64,
    last_update: Instant,
}

impl TokenBucket {
    pub fn new(rate: f64) -> Self {
        Self {
            tokens: rate,
            last_update: Instant::now(),
        }
    }

    pub fn check(&mut self, rate: f64, burst: f64) -> bool {
        let now = Instant::now();
        let elapsed = now.duration_since(self.last_update).as_secs_f64();
        self.tokens = (self.tokens + elapsed * rate).min(burst);
        self.last_update = now;

        if self.tokens >= 1.0 {
            self.tokens -= 1.0;
            true
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn consumes_available_tokens() {
        let mut bucket = TokenBucket::new(1.0);
        assert!(bucket.check(1.0, 1.0));
        assert!(!bucket.check(1.0, 1.0));
    }

    #[test]
    fn refills_after_elapsed_time() {
        let mut bucket = TokenBucket::new(0.0);
        bucket.last_update = Instant::now() - Duration::from_secs(2);
        assert!(bucket.check(1.0, 1.0));
    }
}
