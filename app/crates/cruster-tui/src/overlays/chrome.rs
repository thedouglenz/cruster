//! Shared modal chrome helpers. Keeps every overlay centered + sized
//! against the available frame area with a single rule, so a new
//! modal looks at home next to the existing ones without copy-pasting
//! the geometry.

use ratatui::layout::Rect;

/// Compute a centered rect that fits inside `area`, never larger than
/// `max_w × max_h` and never smaller than `min_w × min_h`. Returns
/// `None` if the frame can't host even the minimum — caller renders
/// nothing in that case rather than crashing.
pub fn centered_rect(area: Rect, min_w: u16, min_h: u16, max_w: u16, max_h: u16) -> Option<Rect> {
    if area.width < min_w || area.height < min_h {
        return None;
    }
    let w = area.width.min(max_w).max(min_w);
    let h = area.height.min(max_h).max(min_h);
    let x = area.x + (area.width - w) / 2;
    let y = area.y + (area.height - h) / 2;
    Some(Rect {
        x,
        y,
        width: w,
        height: h,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn area(w: u16, h: u16) -> Rect {
        Rect {
            x: 0,
            y: 0,
            width: w,
            height: h,
        }
    }

    #[test]
    fn returns_none_when_smaller_than_min() {
        assert!(centered_rect(area(10, 10), 20, 5, 60, 8).is_none());
        assert!(centered_rect(area(80, 4), 20, 5, 60, 8).is_none());
    }

    #[test]
    fn caps_at_max_when_area_is_large() {
        let r = centered_rect(area(200, 60), 20, 5, 60, 8).unwrap();
        assert_eq!((r.width, r.height), (60, 8));
    }

    #[test]
    fn shrinks_to_area_when_area_is_between_min_and_max() {
        let r = centered_rect(area(40, 6), 20, 5, 60, 8).unwrap();
        assert_eq!((r.width, r.height), (40, 6));
    }

    #[test]
    fn centers_within_area() {
        let r = centered_rect(area(100, 20), 20, 5, 60, 8).unwrap();
        assert_eq!(r.x, (100 - 60) / 2);
        assert_eq!(r.y, (20 - 8) / 2);
    }
}
