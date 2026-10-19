/// Largest column count accepted from the view. Larger values are clamped.
pub const MAX_PTY_COLS: u16 = 1024;
/// Largest row count accepted from the view. Larger values are clamped.
pub const MAX_PTY_ROWS: u16 = 512;

/// Reject a zero dimension and clamp an oversized one.
///
/// The PTY itself stores `u16`, so values past the caps above are reduced
/// before they are cast. Zero is rejected because a PTY cannot be resized
/// to an empty screen.
pub fn normalize_pty_size(cols: u32, rows: u32) -> Result<(u16, u16), String> {
    if cols == 0 || rows == 0 {
        return Err("终端尺寸无效".to_string());
    }
    let cols = u16::try_from(cols).unwrap_or(u16::MAX).min(MAX_PTY_COLS);
    let rows = u16::try_from(rows).unwrap_or(u16::MAX).min(MAX_PTY_ROWS);
    Ok((cols, rows))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zero_is_rejected_and_huge_values_are_clamped() {
        assert!(normalize_pty_size(0, 24).is_err());
        assert!(normalize_pty_size(80, 0).is_err());
        assert_eq!(normalize_pty_size(80, 24).unwrap(), (80, 24));
        assert_eq!(
            normalize_pty_size(u32::MAX, u32::MAX).unwrap(),
            (MAX_PTY_COLS, MAX_PTY_ROWS)
        );
        assert_eq!(
            normalize_pty_size(1025, 513).unwrap(),
            (MAX_PTY_COLS, MAX_PTY_ROWS)
        );
    }
}
