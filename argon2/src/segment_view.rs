use crate::{Block, Params};

use core::iter::Iterator;

pub struct SegmentView<'a, F> {
    memory_ptr: *mut Block,
    pass: usize,
    slice: usize,
    lane: usize,
    b: usize,
    prev_index: usize,
    cur_index: usize,
    params: &'a Params,
    rng: F,
}

impl<'a, F> Iterator for SegmentView<'a, F>
where
    F: FnMut(usize, &Block) -> u64,
{
    type Item = (&'a mut Block, &'a Block, &'a Block);

    fn next(&mut self) -> Option<Self::Item> {
        if self.b == self.params.segment_length() {
            return None;
        }

        let cur_block = self.cur_block();
        let prev_block = self.prev_block();

        let rand = (self.rng)(self.b, prev_block);
        let ref_block = self.ref_block(rand);

        self.b += 1;
        self.prev_index = self.cur_index;
        self.cur_index += 1;

        Some((cur_block, prev_block, ref_block))
    }
}

impl<'a, F> SegmentView<'a, F>
where
    F: FnMut(usize, &Block) -> u64,
{
    /// # Safety
    /// - `memory_ptr` must point to the start of a slice of [Block]s, and the slice must have at
    ///   least `params.block_count()` elements
    /// - No other [SegmentView] may exist with the same `pass`, `slice` and `lane`
    pub unsafe fn new(
        memory_ptr: *mut Block,
        pass: usize,
        slice: usize,
        lane: usize,
        params: &'a Params,
        rng: F,
    ) -> Self {
        let first_block = if pass == 0 && slice == 0 {
            // The first two blocks of each lane are already initialized
            2
        } else {
            0
        };

        let cur_index = lane * params.lane_length() + slice * params.segment_length() + first_block;
        let prev_index = if slice == 0 && first_block == 0 {
            // Last block in current lane
            cur_index + params.lane_length() - 1
        } else {
            // Previous block
            cur_index - 1
        };

        Self {
            memory_ptr,
            pass,
            slice,
            lane,
            b: first_block,
            cur_index,
            prev_index,
            params,
            rng,
        }
    }

    fn cur_block(&mut self) -> &'a mut Block {
        unsafe {
            self.memory_ptr
                .add(self.cur_index)
                .as_mut()
                .unwrap_unchecked()
        }
    }

    fn prev_block(&self) -> &'a Block {
        unsafe {
            self.memory_ptr
                .add(self.prev_index)
                .as_ref()
                .unwrap_unchecked()
        }
    }

    fn ref_block(&self, rand: u64) -> &'a Block {
        let ref_lane = if self.pass == 0 && self.slice == 0 {
            // Cannot reference other lanes yet
            self.lane
        } else {
            (rand >> 32) as usize % self.params.lanes()
        };

        let reference_area_size = if self.pass == 0 {
            // First pass
            if self.slice == 0 {
                // First slice, all but the previous
                self.b - 1
            } else if ref_lane == self.lane {
                // The same lane, add current segment
                self.slice * self.params.segment_length() + self.b - 1
            } else {
                self.slice * self.params.segment_length() - if self.b == 0 { 1 } else { 0 }
            }
        } else {
            // Second pass
            if ref_lane == self.lane {
                self.params.lane_length() - self.params.segment_length() + self.b - 1
            } else {
                self.params.lane_length()
                    - self.params.segment_length()
                    - if self.b == 0 { 1 } else { 0 }
            }
        };

        // 1.2.4. Mapping rand to 0..<reference_area_size-1> and produce
        // relative position
        let mut map = rand & 0xFFFFFFFF;
        map = (map * map) >> 32;
        let relative_position =
            reference_area_size - 1 - ((reference_area_size as u64 * map) >> 32) as usize;

        // 1.2.5 Computing starting position
        let start_position = if self.pass != 0 && self.slice != crate::SYNC_POINTS - 1 {
            (self.slice + 1) * self.params.segment_length()
        } else {
            0
        };

        let lane_index = (start_position + relative_position) % self.params.lane_length();
        let ref_index = ref_lane * self.params.lane_length() + lane_index;

        unsafe { self.memory_ptr.add(ref_index).as_ref().unwrap_unchecked() }
    }
}
