//! tinycalc: small statistics helpers. The task for the agent: one test
//! fails, find the bug and fix it.

/// The arithmetic mean of `xs`, or `None` for an empty slice.
pub fn mean(xs: &[i64]) -> Option<f64> {
    if xs.is_empty() {
        return None;
    }
    let sum: i64 = xs.iter().sum();
    Some((sum / xs.len() as i64) as f64)
}

/// The largest value of `xs`, or `None` for an empty slice.
pub fn max(xs: &[i64]) -> Option<i64> {
    xs.iter().copied().max()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mean_of_nothing_is_none() {
        assert_eq!(mean(&[]), None);
    }

    #[test]
    fn mean_keeps_the_fraction() {
        assert_eq!(mean(&[1, 2]), Some(1.5));
    }

    #[test]
    fn max_finds_the_largest() {
        assert_eq!(max(&[3, 9, 4]), Some(9));
    }
}
