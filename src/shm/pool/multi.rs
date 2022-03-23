//! A pool implementation which automatically manage buffers.
//!
//! This pool is built on the [`RawPool`].
//!
//! The [`MultiPool`] takes a key which is used to identify buffers and tries to return the buffer associated to the key
//! if possible. If no buffer in the pool is associated to the key, it will create a new one.
//!
//! # Example
//!
//! ```rust
//! use smithay_client_toolkit::reexports::client::{
//!     ConnectionHandle,
//!     QueueHandle,
//!     protocol::wl_surface::WlSurface,
//!     protocol::wl_shm::Format,
//! };
//! use smithay_client_toolkit::shm::pool::multi::MultiPool;
//!
//! struct WlFoo {
//!     // The surface we'll draw on and the index of buffer associated to it
//!     surface: (WlSurface, usize),
//!     pool: MultiPool<(WlSurface, usize)>
//! }
//!
//! impl WlFoo {
//!     fn draw(&mut self, conn: &mut ConnectionHandle, qh: &QueueHandle<WlFoo>) {
//!         let surface = &self.surface.0;
//!         // We'll increment "i" until the pool can create a new buffer
//!         // if there's no buffer associated with our surface and "i" or if
//!         // a buffer with the obuffer associated with our surface and "i" is free for use.
//!         //
//!         // There's no limit to the amount of buffers we can allocate to our surface but since
//!         // shm buffers are released fairly fast, it's unlikely we'll need more than double buffering.
//!         for i in 0..2 {
//!             self.surface.1 = i;
//!             if let Some((offset, buffer, slice)) = self.pool.create_buffer(
//!                 100,
//!                 100 * 4,
//!                 100,
//!                 &self.surface,
//!                 Format::Argb8888,
//!                 conn,
//!             ) {
//!                 /*
//!                     insert drawing code here
//!                 */
//!                 surface.attach(conn, Some(&buffer), 0, 0);
//!                 surface.commit(conn);
//!                 // We exit the function after the draw.
//!                 return;
//!             }
//!         }
//!         /*
//!             If there's no buffer available we can for example request a frame callback
//!             and trigger a redraw when it fires.
//!             (not shown in this example)
//!         */
//!     }
//! }
//!
//! fn draw(slice: &mut [u8]) {
//!     todo!()
//! }
//!
//! ```
//!

use std::io;

use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use wayland_client::{
    protocol::{wl_buffer, wl_shm, wl_shm_pool},
    ConnectionHandle, Proxy, WEnum,
};
use wayland_client::{Dispatch, QueueHandle};

use crate::shm::ShmState;

use super::raw::RawPool;
use super::CreatePoolError;

/// This pool manages buffers associated with keys.
/// Only one buffer can be attributed to a given key.
#[derive(Debug)]
pub struct MultiPool<K: PartialEq + Clone> {
    buffer_list: Vec<BufferAllocation<K>>,
    pub(crate) inner: RawPool,
}

/// Error associated with buffer access.
#[derive(Debug, thiserror::Error)]
pub enum BufferAccessError {
    /// The buffer is being used by the compositor.
    ///
    /// You will have to wait in order to use this buffer again.
    #[error("the buffer is being used by the compositor")]
    InUse,

    /// The key used to access the buffer is not valid.
    ///
    /// This may happen if the buffer was destroyed or no buffer has been created using the key.
    #[error("the key used to access the buffer is not valid")]
    InvalidKey,
}

/// A buffer allocation within a pool.
#[derive(Debug)]
struct BufferAllocation<K: PartialEq + Clone> {
    /// Whether the compositor has freed the buffer.
    ///
    /// This is `false` when the buffer is in use by the compositor, meaning the buffer contents cannot be
    /// modified and the buffer may not be destroyed.
    free: Arc<AtomicBool>,
    /// Size of the buffer allocation.
    size: usize,
    /// Space used within the buffer allocation.
    ///
    /// This is needed since we over-allocate by 5% to allow small resizes to not need a reallocation.
    used: usize,
    /// Offset within the pool.
    offset: usize,
    /// Protocol object associated with the allocation.
    buffer: Option<wl_buffer::WlBuffer>,
    /// The format of the allocation.
    format: wl_shm::Format,
    key: K,
}

impl<K: PartialEq + Clone> MultiPool<K> {
    /// Resizes the memory pool, notifying the server the pool has changed in size.
    ///
    /// The wl_shm protocol only allows the pool to be made bigger. If the new size is smaller than the
    /// current size of the pool, this function will do nothing.
    pub fn resize(&mut self, size: usize, conn: &mut ConnectionHandle) -> io::Result<()> {
        self.inner.resize(size, conn)
    }

    // TODO: Should return result
    /// Removes the buffer with the given key from the pool and rearranges the others
    pub fn remove(&mut self, key: &K, conn: &mut ConnectionHandle) {
        // TODO: Rearrangement is probably wrong.
        if let Some((i, buffer)) = self.buffer_list.iter().enumerate().find(|&(_, b)| b.key.eq(key))
        {
            let mut offset = buffer.offset;
            self.buffer_list.remove(i);
            for buffer_handle in &mut self.buffer_list {
                if buffer_handle.offset > offset && buffer_handle.free.load(Ordering::Relaxed) {
                    if let Some(buffer) = buffer_handle.buffer.take() {
                        buffer.destroy(conn);
                    }
                    std::mem::swap(&mut buffer_handle.offset, &mut offset);
                } else {
                    break;
                }
            }
        }
    }

    // TODO: Let's just return this was successful and the buffer contents.
    // The actual WlBuffer should not be accessible unless you obtain an `Acquire` which derefs into a
    // WlBuffer. The Acquire should be #[must_use]

    /// Returns the buffer associated with the given key and its offset (usize) in the mempool.
    ///
    /// The offset can be used to determine whether or not a buffer was moved in the mempool
    /// and by consequence if it should be damaged partially or fully.
    ///
    /// When it's not possible to use the buffer associated with the key, None is returned.
    #[forbid(clippy::integer_arithmetic)]
    pub fn create_buffer(
        &mut self,
        width: u32,
        height: u32,
        bytes_per_pixel: u32,
        key: &K,
        format: wl_shm::Format,
        conn: &mut ConnectionHandle,
    ) -> Option<(usize, wl_buffer::WlBuffer, &mut [u8])>
// where
    //     K: std::fmt::Debug,
    {
        // This function must only use checked arithmetic since we want to prevent protocol errors given
        // invalid parameters at release (due to overflow).

        // Since wayland takes i32 for all the parameters, meaning the valid values stop at i32::MAX.
        const MAX: u32 = i32::MAX as u32;

        if width > MAX || height > MAX || bytes_per_pixel > MAX {
            todo!("Invalid params: too large")
        }

        let stride = match width.checked_mul(bytes_per_pixel) {
            Some(stride) if stride > MAX => {
                todo!("invalid params")
            },

            Some(stride) => stride,

            None => todo!("invalid params: too large"),
        };

        // Width may not be larger than the stride. However the width and stride may be equal in the case of
        // 1 by X buffers where the bytes per pixel is 1.
        if width > stride {
            todo!("invalid params")
        }

        let size = match stride.checked_mul(height) {
            Some(size) if size > MAX => {
                todo!("invalid params: too large")
            },

            Some(size) => size,

            None => todo!(),
        };

        let mut found_key = false;
        let mut offset = 0;
        let stride = bytes_per_pixel * width;
        let size = (stride * height) as usize;
        let mut index = None;

        // This loop serves to found the buffer associated to the key.
        for (i, buffer_handle) in self.buffer_list.iter_mut().enumerate() {
            if buffer_handle.key.eq(key) {
                found_key = true;
                if buffer_handle.free.load(Ordering::Relaxed) {
                    // Destroys the buffer if it's resized
                    if size != buffer_handle.used {
                        if let Some(buffer) = buffer_handle.buffer.take() {
                            buffer.destroy(conn);
                        }
                    }
                    // Increases the size of the Buffer if it's too small and add 5% padding.
                    // It is possible this buffer overlaps the following but the else if
                    // statement prevents this buffer from being returned if that's the case.
                    buffer_handle.size = buffer_handle.size.max({
                        let size = size + size / 20;
                        // If the offset isn't a multiple of 4
                        // the client might be unable to use the buffer
                        size + 4 - size % 4
                    });
                    buffer_handle.used = size;
                    index = Some(i);
                }
            // If a buffer is resized, it is likely that the followings might overlap
            } else if offset > buffer_handle.offset {
                // When the buffer is free, it's safe to shift it because we know the compositor won't try to read it.
                if buffer_handle.free.load(Ordering::Relaxed) {
                    if offset != buffer_handle.offset {
                        if let Some(buffer) = buffer_handle.buffer.take() {
                            buffer.destroy(conn);
                        }
                    }
                    buffer_handle.offset = offset;
                } else {
                    // If one of the overlapping buffers is busy, then no buffer can be returned because it could result in a data race.
                    index = None;
                }
            } else if found_key {
                break;
            }
            offset += buffer_handle.size;
        }

        if found_key {
            // Sets the offset to the one of our chosen buffer
            offset = self.buffer_list[index?].offset;
        } else if let Some(b) = self.buffer_list.last() {
            // Adds 5% padding between the last and new buffer
            offset += b.size / 20;
            offset += 4 - offset % 4;
        }

        // Resize the pool if it isn't large enough to fit all our buffers
        if offset + size >= self.inner.len()
            && self.resize(offset + size + size / 20, conn).is_err()
        {
            return None;
        }

        let buffer;

        if found_key {
            let buffer_handle = self.buffer_list.get_mut(index?)?;
            match &buffer_handle.buffer {
                Some(t_buffer) => {
                    buffer = t_buffer.clone();
                }
                None => {
                    let buffer_id = conn
                        .send_request(
                            self.inner.pool(),
                            wl_shm_pool::Request::CreateBuffer {
                                offset: offset as i32,
                                width,
                                height,
                                stride,
                                format: WEnum::Value(format),
                            },
                            Some(Arc::new(BufferObjectData { free: buffer_handle.free.clone() })),
                        )
                        .ok()?;
                    buffer_handle.buffer = Some(Proxy::from_id(conn, buffer_id).ok()?);
                    buffer = buffer_handle.buffer.as_ref()?.clone();
                }
            }
        } else if index.is_none() {
            index = Some(self.buffer_list.len());
            let free = Arc::new(AtomicBool::new(true));
            let buffer_id = conn
                .send_request(
                    self.inner.pool(),
                    wl_shm_pool::Request::CreateBuffer {
                        offset: offset as i32,
                        width,
                        height,
                        stride,
                        format: WEnum::Value(format),
                    },
                    Some(Arc::new(BufferObjectData { free: free.clone() })),
                )
                .ok()?;
            buffer = Proxy::from_id(conn, buffer_id).ok()?;
            self.buffer_list.push(BufferAllocation {
                free,
                size,
                used: 0,
                offset,
                buffer: Some(buffer.clone()),
                format,
                key: key.clone(),
            });
        } else {
            return None;
        }

        let slice = &mut self.inner.mmap()[offset..][..size];

        self.buffer_list[index?].free.swap(false, Ordering::Relaxed);

        Some((offset, buffer, slice))
    }

    pub fn resize_buffer(
        &mut self,
        conn: &mut ConnectionHandle<'_>,
        key: &K,
        width: i32,
        stride: i32,
        height: i32,
    ) -> Result<(), BufferAccessError> {
        // The buffer cannot be resized if it is in use.
        let alloc = self
            .buffer_list
            .iter_mut()
            .find(|alloc| &alloc.key == key)
            .ok_or(BufferAccessError::InvalidKey)?;

        if !alloc.free.load(Ordering::Relaxed) {
            return Err(BufferAccessError::InUse.into());
        }

        let size = (stride * height) as usize;

        // No resize occurs
        if size == alloc.used {
            return Ok(());
        }

        // The buffer is available, check if there is room to expand the buffer within the current allocation.
        let offset = if size > alloc.size {
            // Current allocation is too small, find the first available space in the allocation that fits
            // the required size.

            // Peek forward and see if there is space to expand forward.
            // Otherwise walk further into the memory.
            // No room is available, expand the memory allocation for additional space.
            todo!("find empty space")
        } else {
            // Current allocation is large enough to continue using.
            alloc.offset
        };

        if let Some(buffer) = alloc.buffer.take() {
            buffer.destroy(conn);
        }

        let buffer = todo!();
        alloc.buffer = Some(buffer);

        // TODO

        todo!()
    }

    /// Acquires the content of a buffer, returning a mutable reference to the buffer contents.
    ///
    /// This function may return [`Err`] in one of the following circumstances:
    /// - The buffer is still in use by the compositor.
    /// - The key is not associated with a buffer allocation.
    ///
    /// Note that changing the content of a buffer does not cause presentation. You will need to attach the
    /// buffer to a `WlSurface` and commit the surface to cause presentation.
    pub fn acquire_buffer(&mut self, key: &K) -> Result<&mut [u8], BufferAccessError> {
        let alloc = self
            .buffer_list
            .iter()
            .find(|alloc| &alloc.key == key)
            .ok_or(BufferAccessError::InvalidKey)?;

        if !alloc.free.load(Ordering::Relaxed) {
            return Err(BufferAccessError::InUse);
        }

        let buffer = &mut self.inner.mmap()[alloc.offset..][..alloc.size];
        // TODO: This is wrong to lock here, but we have no way to assert the user has made the buffer
        // inaccessible while the compositor has the buffer?
        alloc.free.swap(false, Ordering::Relaxed);

        Ok(buffer)
    }
}

impl ShmState {
    pub fn new_multi<D, K, U>(
        &self,
        conn: &mut ConnectionHandle<'_>,
        qh: &QueueHandle<D>,
        udata: U,
    ) -> Result<MultiPool<K>, CreatePoolError>
    where
        D: Dispatch<wl_shm_pool::WlShmPool, UserData = U> + 'static,
        K: Clone + PartialEq,
        U: Send + Sync + 'static,
    {
        Ok(MultiPool { buffer_list: Vec::new(), inner: self.new_raw_pool(0, conn, qh, udata)? })
    }
}

struct BufferObjectData {
    free: Arc<AtomicBool>,
}

impl wayland_client::backend::ObjectData for BufferObjectData {
    fn event(
        self: Arc<Self>,
        _: &mut wayland_backend::client::Handle,
        msg: wayland_backend::protocol::Message<wayland_backend::client::ObjectId>,
    ) -> Option<Arc<dyn wayland_backend::client::ObjectData>> {
        debug_assert!(wayland_client::backend::protocol::same_interface(
            msg.sender_id.interface(),
            wl_buffer::WlBuffer::interface()
        ));
        // wl_buffer only has a single event: wl_buffer.release
        debug_assert_eq!(msg.opcode, 0);
        self.free.store(true, Ordering::Relaxed);
        None
    }

    fn destroyed(&self, _: wayland_backend::client::ObjectId) {}
}
