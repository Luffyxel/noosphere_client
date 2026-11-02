use super::{Image, Layout, error};
use crate::Result;
use wayland_client::protocol::{wl_output::Transform, wl_shm::Format};

pub(super) fn convert(
    mut data: Vec<u8>,
    layout: Layout,
    inverted: bool,
    transform: Transform,
) -> Result<Image> {
    let width = layout.width as usize;
    let height = layout.height as usize;
    let row = width * 4;
    for y in 0..height {
        let start = y * layout.stride as usize;
        data.copy_within(start..start + row, y * row);
    }
    data.truncate(row * height);
    if !cfg!(target_endian = "little")
        || !matches!(layout.format, Format::Xrgb8888 | Format::Argb8888)
    {
        for pixel in data.chunks_exact_mut(4) {
            let value = u32::from_ne_bytes(pixel.try_into().unwrap());
            let (red, green, blue) = match layout.format {
                Format::Xrgb8888 | Format::Argb8888 => (value >> 16, value >> 8, value),
                Format::Xbgr8888 | Format::Abgr8888 => (value, value >> 8, value >> 16),
                Format::Xrgb2101010 | Format::Argb2101010 => (value >> 22, value >> 12, value >> 2),
                Format::Xbgr2101010 | Format::Abgr2101010 => (value >> 2, value >> 12, value >> 22),
                _ => {
                    return Err(error(
                        "format de capture",
                        format!("format non pris en charge : {:?}", layout.format),
                    ));
                }
            };
            pixel.copy_from_slice(&[blue as u8, green as u8, red as u8, 255]);
        }
    }
    if inverted {
        for y in 0..height / 2 {
            let (top, bottom) = data.split_at_mut((height - 1 - y) * row);
            top[y * row..(y + 1) * row].swap_with_slice(&mut bottom[..row]);
        }
    }
    if transform == Transform::Normal {
        return Ok(Image {
            pixels: data,
            width: width as u32,
            height: height as u32,
        });
    }
    let rotated = matches!(
        transform,
        Transform::_90 | Transform::_270 | Transform::Flipped90 | Transform::Flipped270
    );
    let (out_width, out_height) = if rotated {
        (height, width)
    } else {
        (width, height)
    };
    let mut pixels = vec![0; data.len()];
    for y in 0..height {
        for x in 0..width {
            let (out_x, out_y) = match transform {
                Transform::_90 => (y, width - 1 - x),
                Transform::_180 => (width - 1 - x, height - 1 - y),
                Transform::_270 => (height - 1 - y, x),
                Transform::Flipped => (width - 1 - x, y),
                Transform::Flipped90 => (y, x),
                Transform::Flipped180 => (x, height - 1 - y),
                Transform::Flipped270 => (height - 1 - y, width - 1 - x),
                _ => return Err(error("orientation de capture", "transformation inconnue")),
            };
            let from = (y * width + x) * 4;
            let to = (out_y * out_width + out_x) * 4;
            pixels[to..to + 4].copy_from_slice(&data[from..from + 4]);
        }
    }
    Ok(Image {
        pixels,
        width: out_width as u32,
        height: out_height as u32,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packed_rows_ignore_padding_and_restore_inverted_buffers() {
        let layout = Layout {
            format: Format::Xrgb8888,
            width: 2,
            height: 2,
            stride: 12,
        };
        let data = vec![
            1, 2, 3, 0, 4, 5, 6, 0, 99, 99, 99, 99, 7, 8, 9, 0, 10, 11, 12, 0, 99, 99, 99, 99,
        ];
        let image = convert(data, layout, true, Transform::Normal).unwrap();
        assert_eq!(
            image.pixels,
            vec![7, 8, 9, 0, 10, 11, 12, 0, 1, 2, 3, 0, 4, 5, 6, 0]
        );
    }

    #[test]
    fn portrait_and_mirrored_monitors_keep_pixel_positions() {
        let layout = Layout {
            format: Format::Xrgb8888,
            width: 3,
            height: 2,
            stride: 12,
        };
        let data: Vec<_> = (1..=6).flat_map(|blue| [blue, 0, 0, 0]).collect();
        for (transform, expected) in [
            (Transform::_90, vec![3, 6, 2, 5, 1, 4]),
            (Transform::_270, vec![4, 1, 5, 2, 6, 3]),
            (Transform::Flipped90, vec![1, 4, 2, 5, 3, 6]),
        ] {
            let image = convert(data.clone(), layout, false, transform).unwrap();
            assert_eq!((image.width, image.height), (2, 3));
            assert_eq!(
                image
                    .pixels
                    .chunks_exact(4)
                    .map(|pixel| pixel[0])
                    .collect::<Vec<_>>(),
                expected
            );
        }
    }

    #[test]
    fn ten_bit_outputs_convert_to_eight_bit_without_swapping_channels() {
        let layout = Layout {
            format: Format::Xrgb2101010,
            width: 1,
            height: 1,
            stride: 4,
        };
        let image = convert(
            (1023u32 << 20 | 512 << 10).to_ne_bytes().to_vec(),
            layout,
            false,
            Transform::Normal,
        )
        .unwrap();
        assert_eq!(image.pixels, vec![0, 128, 255, 255]);
    }
}
