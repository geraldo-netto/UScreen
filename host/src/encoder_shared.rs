//! AVFrame adapter retaining a read-only shared slot until the last AVBufferRef.
use crate::raw_memory::Lease;
use anyhow::{ensure, Result};
use ffmpeg_next::{ffi, frame::Video};

pub(super) enum Input {
    Fifo {
        reader: super::FifoReader<std::fs::File>,
        input: super::input_frame::RawInput,
    },
    Shared(crate::raw_memory::Reader),
}

impl Input {
    pub fn open(
        path: &std::path::Path,
        socket: Option<crate::raw_socket::Socket>,
        dimensions: (u32, u32),
        slots: u32,
    ) -> Result<Self> {
        if let Some(socket) = socket {
            return Ok(Self::Shared(crate::raw_memory::Reader::new(
                socket, dimensions, slots,
            )?));
        }
        // Nonblocking open lets cancellation work even without an active tablet.
        use std::os::unix::fs::OpenOptionsExt;
        let fifo = std::fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NONBLOCK | libc::O_NOFOLLOW)
            .open(path)?;
        Ok(Self::Fifo {
            reader: super::FifoReader::new(fifo),
            input: Default::default(),
        })
    }

    pub fn read(&mut self, frame: &mut Video, stop: &super::StopSignal) -> Result<bool> {
        match self {
            Self::Fifo { reader, input } => input.read(frame, reader, stop),
            Self::Shared(reader) => {
                let Some(lease) = reader.next(stop)? else {
                    return Ok(false);
                };
                *frame = wrap(lease)?;
                Ok(true)
            }
        }
    }
}

unsafe extern "C" fn release_slot(opaque: *mut libc::c_void, _data: *mut u8) {
    // This is the only owner of the box after av_buffer_create succeeds.
    drop(Box::from_raw(opaque.cast::<Lease>()));
}

pub(super) fn wrap(lease: Lease) -> Result<Video> {
    let layout = lease.message.layout;
    let pixels = lease.bytes().as_ptr().cast_mut();
    let owner = Box::into_raw(Box::new(lease));
    unsafe {
        let buffer = ffi::av_buffer_create(
            pixels,
            layout.frame_bytes(),
            Some(release_slot),
            owner.cast(),
            ffi::AV_BUFFER_FLAG_READONLY,
        );
        if buffer.is_null() {
            drop(Box::from_raw(owner));
            anyhow::bail!("allocate shared AVBufferRef");
        }
        let mut frame = Video::empty();
        let raw = frame.as_mut_ptr();
        // Video::empty owns an AVFrame. ffmpeg-next follows av_frame_alloc's
        // allocation convention; validate before assigning the buffer reference.
        if raw.is_null() {
            let mut buffer = buffer;
            ffi::av_buffer_unref(&mut buffer);
        }
        ensure!(!raw.is_null(), "allocate shared AVFrame");
        (*raw).buf[0] = buffer;
        (*raw).data[0] = pixels;
        (*raw).data[1] = pixels.add(layout.uv_offset as usize);
        (*raw).linesize[0] = layout.stride as i32;
        (*raw).linesize[1] = layout.stride as i32;
        frame.set_width(layout.width);
        frame.set_height(layout.height);
        frame.set_format(ffmpeg_next::format::Pixel::NV12);
        frame.set_color_space(ffmpeg_next::color::Space::BT709);
        frame.set_color_range(ffmpeg_next::color::Range::MPEG);
        frame.set_color_primaries(ffmpeg_next::color::Primaries::BT709);
        frame.set_color_transfer_characteristic(ffmpeg_next::color::TransferCharacteristic::BT709);
        Ok(frame)
    }
}
