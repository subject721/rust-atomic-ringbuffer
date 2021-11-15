use std::marker;
use std::marker::PhantomData;
use std::mem::MaybeUninit;
use std::ops::Deref;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

#[allow(dead_code)]

struct SharedBufferState<T: Sized> {
    ring_capacity: u64,
    element_size: u64,

    wr_index: AtomicU64,
    rd_index: AtomicU64,

    storage: bytes::BytesMut,

    _marker: marker::PhantomData<T>,
}

pub struct BufferWriter<T: Sized> {
    shared_state: Arc<SharedBufferState<T>>,
}

pub struct BufferReader<T: Sized> {
    shared_state: Arc<SharedBufferState<T>>,
}

impl<T: Sized> SharedBufferState<T> {
    pub fn size(&self) -> usize {
        let cur_read_idx = self.rd_index.load(Ordering::Acquire);
        let cur_write_idx = self.wr_index.load(Ordering::Acquire);

        ((cur_write_idx + self.ring_capacity - cur_read_idx) % self.ring_capacity) as usize
    }

    pub fn capacity(&self) -> usize {
        self.ring_capacity as usize
    }
}

impl<T: Sized> BufferWriter<T> {
    pub fn size(&self) -> usize {
        let state = self.shared_state.deref();

        state.size()
    }

    pub fn capacity(&self) -> usize {
        let state = self.shared_state.deref();

        state.capacity()
    }

    pub fn try_write(&mut self, value: T) -> Result<(), T> {
        let mut v = MaybeUninit::new(value);

        let state = self.shared_state.deref();

        let cur_read_idx = state.rd_index.load(Ordering::Acquire);
        let cur_write_idx = state.wr_index.load(Ordering::Acquire);

        if ((cur_write_idx + state.ring_capacity - cur_read_idx) % state.ring_capacity)
            == (state.ring_capacity - 1)
        {
            return Err(unsafe { v.assume_init() });
        }

        unsafe {
            let dst_ptr = state
                .storage
                .as_ptr()
                .offset((cur_write_idx * state.element_size) as isize)
                as *mut T;

            std::mem::swap(&mut *v.as_mut_ptr(), &mut *dst_ptr);
        }

        state
            .wr_index
            .store((cur_write_idx + 1) % state.ring_capacity, Ordering::Release);

        Ok(())
    }
}

impl<T: Sized> BufferReader<T> {
    pub fn size(&self) -> usize {
        let state = self.shared_state.deref();

        state.size()
    }

    pub fn capacity(&self) -> usize {
        let state = self.shared_state.deref();

        state.capacity()
    }

    pub fn try_read(&mut self) -> Option<T> {
        let state = self.shared_state.deref();

        let cur_read_idx = state.rd_index.load(Ordering::Acquire);
        let cur_write_idx = state.wr_index.load(Ordering::Acquire);

        if cur_read_idx == cur_write_idx {
            return Option::None;
        }

        let ret = unsafe {
            let src_ptr = state
                .storage
                .as_ptr()
                .offset((cur_read_idx * state.element_size) as isize)
                as *mut T;

            let mut v = MaybeUninit::uninit();

            std::mem::swap(&mut *src_ptr, &mut *v.as_mut_ptr());

            Option::Some(v.assume_init())
        };

        state
            .rd_index
            .store((cur_read_idx + 1) % state.ring_capacity, Ordering::Release);

        ret
    }
}

impl<T> Drop for BufferReader<T> {
    fn drop(&mut self) {
        while self.try_read().is_some() {};
    }
}

fn size_align(type_size: usize, min_alignment: usize) -> usize {
    let mut tmp = type_size / min_alignment;

    if (type_size % min_alignment) != 0 {
        tmp += 1;
    }

    tmp * min_alignment
}

pub fn create_ring_buffer<T: Sized>(
    buffer_capacity: usize,
) -> (BufferWriter<T>, BufferReader<T>) {

    let actual_buffer_capacity = if buffer_capacity < 2 {
        2
    } else {
        buffer_capacity
    };

    let element_size = size_align(std::mem::size_of::<T>(), std::mem::align_of::<*const T>());

    let storage = bytes::BytesMut::with_capacity(element_size * actual_buffer_capacity);

    let shared_state = Arc::new(SharedBufferState {
        ring_capacity: actual_buffer_capacity as u64,
        element_size: element_size as u64,
        wr_index: AtomicU64::new(0),
        rd_index: AtomicU64::new(0),
        storage,
        _marker: PhantomData::default(),
    });

    (
        BufferWriter {
            shared_state: shared_state.clone(),
        },
        BufferReader { shared_state },
    )
}

#[cfg(test)]
mod tests {
    use crate::create_ring_buffer;

    #[test]
    fn basic_creation_test() {
        let (buffer_writer, buffer_reader) = create_ring_buffer::<i32>(12);

        assert_eq!(buffer_writer.capacity(), 12);
        assert_eq!(buffer_reader.capacity(), 12);

        assert_eq!(buffer_writer.size(), 0);
        assert_eq!(buffer_reader.size(), 0);
    }

    #[test]
    fn basic_element_test() {
        let (mut buffer_writer, mut buffer_reader) = create_ring_buffer::<u32>(12);

        assert!(buffer_writer.try_write(1337u32).is_ok());

        assert_eq!(buffer_writer.size(), 1);
        assert_eq!(buffer_reader.size(), 1);

        let read_item1 = buffer_reader.try_read();
        let read_item2 = buffer_reader.try_read();

        assert!(read_item1.is_some());
        assert!(read_item2.is_none());

        assert_eq!(read_item1.unwrap(), 1337u32);
    }

    #[test]
    fn basic_element_test2() {
        let (mut buffer_writer, mut buffer_reader) = create_ring_buffer::<u32>(2);

        assert!(buffer_writer.try_write(1u32).is_ok());
        assert!(buffer_writer.try_write(2u32).is_err());

        assert_eq!(buffer_writer.size(), 1);
        assert_eq!(buffer_reader.size(), 1);

        let read_item1 = buffer_reader.try_read();
        let read_item2 = buffer_reader.try_read();

        assert!(read_item1.is_some());
        assert!(read_item2.is_none());

        assert_eq!(read_item1.unwrap(), 1u32);
    }

    #[derive(Clone)]
    struct SomeElementType {
        s: String,
        v: u32,
    }

    #[test]
    fn basic_element_test3() {
        let (mut buffer_writer, mut buffer_reader) = create_ring_buffer::<SomeElementType>(
            2
        );

        let new_elem1 = SomeElementType {
            s: String::from("Element1"),
            v: 1337,
        };

        let new_elem2 = SomeElementType {
            s: String::from("Element2"),
            v: 1338,
        };

        assert!(buffer_writer.try_write(new_elem1).is_ok());
        assert!(buffer_writer.try_write(new_elem2).is_err());

        assert_eq!(buffer_writer.size(), 1);
        assert_eq!(buffer_reader.size(), 1);

        let read_item1 = buffer_reader.try_read();
        let read_item2 = buffer_reader.try_read();

        assert!(read_item1.is_some());
        assert!(read_item2.is_none());

        assert_eq!(read_item1.unwrap().s, "Element1");
    }

    #[test]
    fn threaded_test1() {}
}
