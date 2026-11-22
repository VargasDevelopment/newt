// SPDX-License-Identifier: MIT
//! Borrowed frozen CoverObservedHeap view; no allocation or metric substitution.
use super::observation::Observation;
use newt_native_io::cover as native;
pub struct Heap<'a>{observation:Option<&'a Observation>}
impl<'a> Heap<'a>{
    pub fn new(observation:Option<&'a Observation>)->Self{Self{observation}}
    pub fn observation(&self)->Option<&'a Observation>{self.observation}
    pub fn free_bytes(&self)->usize{native::free_bytes()}
    pub fn largest(&self)->usize{
        let actual=native::largest();
        if let Some(observation)=self.observation{observation.sample_largest(actual);}
        actual
    }
}
