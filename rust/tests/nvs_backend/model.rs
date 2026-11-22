// SPDX-License-Identifier: MIT

//! Independent scripted native faults, not an NVS durability emulator.
use crate::nvs::{Native, NativeError};
use std::ffi::CStr;

#[derive(Clone)]
pub struct Case {
    pub name: String,
    pub op: String,
    pub ready: bool,
    pub readonly: bool,
    pub open: i32,
    pub query: i32,
    pub query_size: usize,
    pub data: i32,
    pub data_size: usize,
    pub set: i32,
    pub commit: i32,
    pub erase: i32,
}
impl Case {
    pub fn parse(line: &str) -> Self {
        let fields: Vec<_> = line.split_whitespace().collect();
        assert_eq!(fields.len(), 12);
        Self {
            name: fields[0].into(),
            op: fields[1].into(),
            ready: fields[2] == "1",
            readonly: fields[3] == "1",
            open: fields[4].parse().unwrap(),
            query: fields[5].parse().unwrap(),
            query_size: fields[6].parse().unwrap(),
            data: fields[7].parse().unwrap(),
            data_size: fields[8].parse().unwrap(),
            set: fields[9].parse().unwrap(),
            commit: fields[10].parse().unwrap(),
            erase: fields[11].parse().unwrap(),
        }
    }
}

pub struct Model {
    pub case: Case,
    pub trace: String,
    pub record: [u8; 4],
    pub durable: bool,
    ready_calls: usize,
    open_calls: usize,
}
impl Model {
    pub fn new(case: Case) -> Self {
        Self {
            case,
            trace: String::new(),
            record: [11, 22, 33, 44],
            durable: true,
            ready_calls: 0,
            open_calls: 0,
        }
    }
    fn log(&mut self, value: String) {
        self.trace.push_str(&value);
        self.trace.push(';');
    }
}
impl Native for Model {
    fn storage_ready(&mut self) -> bool {
        self.ready_calls += 1;
        let ready = self.case.ready && !(self.case.op == "regate" && self.ready_calls > 1);
        self.log(format!("ready:{}", u8::from(ready)));
        ready
    }
    fn open(&mut self, name: &CStr, readonly: bool, handle: &mut u32) -> Result<(), NativeError> {
        self.open_calls += 1;
        let code = if self.case.op == "reopenfail" && self.open_calls > 1 {
            0x1102
        } else {
            self.case.open
        };
        self.log(format!(
            "open:{}:{}",
            name.to_str().unwrap(),
            u8::from(readonly)
        ));
        *handle = if code == 0 { 7 } else { 19 };
        NativeError::from_code(code)
    }
    fn close(&mut self, handle: u32) {
        self.log(format!("close:{handle}"));
    }
    fn get_blob(
        &mut self,
        handle: u32,
        key: &CStr,
        output: Option<&mut [u8]>,
        size: &mut usize,
    ) -> Result<(), NativeError> {
        match output {
            None => {
                self.log(format!("query:{handle}:{}:{size}", key.to_str().unwrap()));
                *size = self.case.query_size;
                NativeError::from_code(self.case.query)
            }
            Some(output) => {
                self.log(format!("data:{handle}:{}:{size}", key.to_str().unwrap()));
                assert!(*size <= output.len());
                if self.case.data == 0 {
                    let copied = output.len().min(self.record.len()).min(self.case.data_size);
                    output[..copied].copy_from_slice(&self.record[..copied]);
                }
                *size = self.case.data_size;
                NativeError::from_code(self.case.data)
            }
        }
    }
    fn set_blob(&mut self, handle: u32, key: &CStr, bytes: &[u8]) -> Result<(), NativeError> {
        self.log(format!(
            "set:{handle}:{}:{}",
            key.to_str().unwrap(),
            bytes.len()
        ));
        if self.case.set == 0 {
            self.record.copy_from_slice(bytes);
        }
        NativeError::from_code(self.case.set)
    }
    fn commit(&mut self, handle: u32) -> Result<(), NativeError> {
        self.log(format!("commit:{handle}"));
        NativeError::from_code(self.case.commit)
    }
    fn erase_key(&mut self, handle: u32, key: &CStr) -> Result<(), NativeError> {
        self.log(format!("erase:{handle}:{}", key.to_str().unwrap()));
        NativeError::from_code(self.case.erase)
    }
    fn readback_durable(&self) -> bool {
        self.durable
    }
}
