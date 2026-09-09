use tauri::{PhysicalPosition, PhysicalSize, WebviewWindow};
use tauri_plugin_window_state::StateFlags;

// Visibility belongs to the splash/startup flow, not to the saved window state.
pub const FLAGS: StateFlags = StateFlags::SIZE
    .union(StateFlags::POSITION)
    .union(StateFlags::MAXIMIZED);

pub fn ensure_reachable(window: &WebviewWindow) -> tauri::Result<()> {
    if window.is_maximized()? || window.is_fullscreen()? {
        return Ok(());
    }
    let position = window.outer_position()?;
    let size = window.outer_size()?;
    let monitors = window.available_monitors()?;
    if monitors.is_empty() {
        return Ok(());
    }
    let visible_monitor = monitors.iter().find(|monitor| {
        let area = monitor.work_area();
        title_bar_visible(
            (position.x, position.y),
            size.width,
            (
                area.position.x,
                area.position.y,
                area.size.width,
                area.size.height,
            ),
        )
    });
    if visible_monitor.is_some_and(|monitor| {
        let area = monitor.work_area();
        size_fits(
            (size.width, size.height),
            (area.size.width, area.size.height),
        )
    }) {
        return Ok(());
    }
    let monitor = match visible_monitor {
        Some(monitor) => monitor.clone(),
        None => window
            .primary_monitor()?
            .unwrap_or_else(|| monitors[0].clone()),
    };
    let area = monitor.work_area();
    let inner = window.inner_size()?;
    window.set_size(PhysicalSize::new(
        inner.width.min(
            area.size
                .width
                .saturating_sub(size.width.saturating_sub(inner.width)),
        ),
        inner.height.min(
            area.size
                .height
                .saturating_sub(size.height.saturating_sub(inner.height)),
        ),
    ))?;
    let size = window.outer_size()?;
    let (x, y) = centered_position(
        (
            area.position.x,
            area.position.y,
            area.size.width,
            area.size.height,
        ),
        (size.width, size.height),
    );
    window.set_position(PhysicalPosition::new(x, y))
}

fn size_fits(size: (u32, u32), area: (u32, u32)) -> bool {
    size.0 <= area.0 && size.1 <= area.1
}

fn centered_position(area: (i32, i32, u32, u32), size: (u32, u32)) -> (i32, i32) {
    (
        area.0
            .saturating_add((area.2.saturating_sub(size.0) / 2) as i32),
        area.1
            .saturating_add((area.3.saturating_sub(size.1) / 2) as i32),
    )
}

fn title_bar_visible(position: (i32, i32), width: u32, area: (i32, i32, u32, u32)) -> bool {
    let (x, y) = (i64::from(position.0), i64::from(position.1));
    let (left, top) = (i64::from(area.0), i64::from(area.1));
    let overlap = (x + i64::from(width)).min(left + i64::from(area.2)) - x.max(left);
    y >= top && y + 32 <= top + i64::from(area.3) && overlap >= 128
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn restored_windows_remain_reachable_across_monitor_changes() {
        let primary = (0, 0, 1920, 1040);
        let left_monitor = (-2560, -400, 2560, 1440);
        assert!(title_bar_visible((200, 120), 1280, primary));
        assert!(size_fits((1280, 800), (primary.2, primary.3)));
        assert!(!size_fits((1280, 1600), (primary.2, primary.3)));
        assert!(!size_fits((2560, 800), (primary.2, primary.3)));
        assert!(title_bar_visible((-2400, -200), 1280, left_monitor));
        assert!(!title_bar_visible((-2400, -200), 1280, primary));
        assert!(!title_bar_visible((200, -500), 1280, primary));
        assert!(!title_bar_visible((1900, 120), 1280, primary));
        assert!(!title_bar_visible((200, 1030), 1280, primary));
        assert!(title_bar_visible((-1100, 120), 1280, primary));
        assert_eq!(
            centered_position((-1920, -1080, 1920, 1080), (1280, 1600)),
            (-1600, -1080)
        );
        let recovered = centered_position(primary, (1280, 1600));
        assert!(title_bar_visible(recovered, 1280, primary));
    }

    #[test]
    fn restoring_geometry_preserves_the_splash_startup_contract() {
        let config: serde_json::Value =
            serde_json::from_str(include_str!("../tauri.conf.json")).unwrap();
        let windows = config["app"]["windows"].as_array().unwrap();
        let main = windows
            .iter()
            .find(|window| window["label"] == "main")
            .unwrap();
        assert_eq!(main["center"], true);
        assert_eq!(main["visible"], false);
        assert!(FLAGS.contains(StateFlags::SIZE | StateFlags::POSITION | StateFlags::MAXIMIZED));
        assert!(
            !FLAGS
                .intersects(StateFlags::VISIBLE | StateFlags::FULLSCREEN | StateFlags::DECORATIONS)
        );
    }
}
