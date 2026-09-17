//! Writable NV12 input ownership, separate from codec submission and output.
use crate::encoder_io::{read_frame, read_frame_into, FrameSource, FrameTarget, StopSignal};
use anyhow::{Context, Result};
use ffmpeg_next::frame::Video;

#[derive(Default)]
pub(super) struct RawInput {
    staging: Vec<u8>,
}

impl RawInput {
    pub fn read(
        &mut self,
        frame: &mut Video,
        source: &mut impl FrameSource,
        stop: &StopSignal,
    ) -> Result<bool> {
        writable(frame)?;
        let width = frame.width() as usize;
        let y_size = width * frame.height() as usize;
        if frame.stride(0) == width && frame.stride(1) == width {
            return Ok(read_frame_into(
                source,
                &mut Planes { frame, y_size },
                stop,
            )?);
        }
        // Padded widths retain packed staging: a read per short row can add
        // thousands of syscalls. Ordinary aligned modes need neither this
        // allocation nor a userspace input copy.
        self.staging.resize(y_size * 3 / 2, 0);
        if !read_frame(source, &mut self.staging, stop)? {
            return Ok(false);
        }
        copy_nv12(frame, &self.staging);
        Ok(true)
    }
}

struct Planes<'a> {
    frame: &'a mut Video,
    y_size: usize,
}
impl FrameTarget for Planes<'_> {
    fn len(&self) -> usize {
        self.y_size * 3 / 2
    }
    fn chunk(&mut self, offset: usize) -> &mut [u8] {
        if offset < self.y_size {
            &mut self.frame.data_mut(0)[offset..self.y_size]
        } else {
            &mut self.frame.data_mut(1)[offset - self.y_size..self.y_size / 2]
        }
    }
}

pub(super) fn writable(frame: &mut Video) -> Result<()> {
    // Detach retained references before acquiring mutable planes/strides.
    let result = unsafe { ffmpeg_next::ffi::av_frame_make_writable(frame.as_mut_ptr()) };
    if result < 0 {
        return Err(ffmpeg_next::Error::from(result)).context("make encoder frame writable");
    }
    Ok(())
}

pub(super) fn copy_nv12(frame: &mut Video, nv12: &[u8]) {
    let (width, height) = (frame.width() as usize, frame.height() as usize);
    let (y_stride, uv_stride) = (frame.stride(0), frame.stride(1));
    copy_plane(
        frame.data_mut(0),
        y_stride,
        &nv12[..width * height],
        width,
        height,
    );
    copy_plane(
        frame.data_mut(1),
        uv_stride,
        &nv12[width * height..],
        width,
        height / 2,
    );
}

pub(super) fn copy_plane(dst: &mut [u8], stride: usize, src: &[u8], width: usize, rows: usize) {
    if stride == width && width != 0 {
        // Preserve the previous complete-row behavior even for short slices.
        let count = rows.min(dst.len() / width).min(src.len() / width) * width;
        dst[..count].copy_from_slice(&src[..count]);
        return;
    }
    for row in 0..rows {
        let d = row * stride;
        let s = row * width;
        if d + width <= dst.len() && s + width <= src.len() {
            dst[d..d + width].copy_from_slice(&src[s..s + width]);
        }
    }
}

#[cfg(test)]
#[path = "encoder_frame_tests.rs"]
mod tests;
