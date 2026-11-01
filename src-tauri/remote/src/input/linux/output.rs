#[derive(Clone, Debug)]
pub(super) struct OutputGeometry {
    pub name: Option<String>,
    pub mode: Option<(i32, i32)>,
    pub scale: i32,
    pub rotated: bool,
}

impl Default for OutputGeometry {
    fn default() -> Self {
        Self {
            name: None,
            mode: None,
            scale: 1,
            rotated: false,
        }
    }
}

impl OutputGeometry {
    fn matches_size(&self, size: (i32, i32)) -> bool {
        let Some((width, height)) = self.mode.filter(|(w, h)| *w > 0 && *h > 0) else {
            return false;
        };
        let rotated = if self.rotated {
            (height, width)
        } else {
            (width, height)
        };
        size == rotated || size == (rotated.0 / self.scale.max(1), rotated.1 / self.scale.max(1))
    }
}

pub(super) fn select_output(
    outputs: &[OutputGeometry],
    mapping_id: Option<&str>,
    size: Option<(i32, i32)>,
) -> Option<usize> {
    if let Some(name) = mapping_id.filter(|name| !name.is_empty()) {
        return unique(
            outputs
                .iter()
                .enumerate()
                .filter(|(_, output)| output.name.as_deref() == Some(name))
                .map(|(index, _)| index),
        );
    }
    if outputs.len() == 1 {
        return Some(0);
    }
    // Some portals report (0, 0) for every monitor. Never choose by position alone.
    let size = size?;
    unique(
        outputs
            .iter()
            .enumerate()
            .filter(|(_, output)| output.matches_size(size))
            .map(|(index, _)| index),
    )
}

fn unique(mut indices: impl Iterator<Item = usize>) -> Option<usize> {
    let first = indices.next()?;
    indices.next().is_none().then_some(first)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn output(name: &str, mode: (i32, i32), scale: i32, rotated: bool) -> OutputGeometry {
        OutputGeometry {
            name: Some(name.into()),
            mode: Some(mode),
            scale,
            rotated,
        }
    }

    #[test]
    fn selects_the_named_monitor_even_when_resolutions_match() {
        let outputs = [
            output("DP-1", (2560, 1440), 1, true),
            output("DP-2", (2560, 1440), 1, false),
        ];
        assert_eq!(
            select_output(&outputs, Some("DP-2"), Some((2560, 1440))),
            Some(1)
        );
        assert_eq!(
            select_output(&outputs, Some("DP-1"), Some((2560, 1440))),
            Some(0)
        );
    }

    #[test]
    fn matches_scaled_and_portrait_outputs_without_assuming_desktop_origin() {
        let outputs = [
            output("eDP-1", (3840, 2160), 2, false),
            output("DP-1", (2560, 1440), 1, true),
        ];
        assert_eq!(select_output(&outputs, None, Some((1920, 1080))), Some(0));
        assert_eq!(select_output(&outputs, None, Some((3840, 2160))), Some(0));
        assert_eq!(select_output(&outputs, None, Some((1440, 2560))), Some(1));
    }

    #[test]
    fn refuses_ambiguous_or_disconnected_monitors() {
        let outputs = [
            output("DP-1", (1920, 1080), 1, false),
            output("DP-2", (1920, 1080), 1, false),
        ];
        assert_eq!(select_output(&outputs, None, Some((1920, 1080))), None);
        assert_eq!(
            select_output(&outputs, Some("DP-3"), Some((1920, 1080))),
            None
        );
        assert_eq!(select_output(&outputs, None, None), None);
        assert_eq!(select_output(&[], None, None), None);
    }

    #[test]
    fn accepts_a_single_monitor_without_optional_portal_metadata() {
        assert_eq!(
            select_output(&[OutputGeometry::default()], None, None),
            Some(0)
        );
    }
}
