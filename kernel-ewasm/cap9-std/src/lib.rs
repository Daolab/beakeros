#![no_std]
#![allow(unused_imports)]
#![allow(dead_code)]

extern crate pwasm_abi;
use pwasm_abi::types::*;
use cap9_core::Serialize;

/// Generic wasm error
#[derive(Debug)]
pub struct Error;

pub mod proc_table;
pub mod syscalls;
pub use syscalls::*;

// Re-export pwasm::Vec as the Vec type for cap9_std
pub use pwasm_std::Vec;

// When we are compiling to WASM, unresolved references are left as (import)
// expressions. However, under any other target symbols will have to be linked
// for EVM functions (blocknumber, create, etc.). Therefore, when we are not
// compiling for WASM (be it test, realse, whatever) we want to link in dummy
// functions. pwasm_test provides all the builtins provided by parity, while
// cap9_test covers the few that we have implemented ourselves.
#[cfg(not(target_arch = "wasm32"))]
extern crate pwasm_test;
#[cfg(not(target_arch = "wasm32"))]
extern crate cap9_test;

/// TODO: this is duplicated from pwasm_ethereum as it is currently in a private
/// module.
pub mod external {
    extern "C" {
        pub fn extcodesize( address: *const u8) -> i32;
        pub fn extcodecopy( dest: *mut u8, address: *const u8);
        pub fn dcall(
                gas: i64,
                address: *const u8,
                input_ptr: *const u8,
                input_len: u32,
                result_ptr: *mut u8,
                result_len: u32,
        ) -> i32;

        pub fn call_code(
                gas: i64,
                address: *const u8,
                val_ptr: *const u8,
                input_ptr: *const u8,
                input_len: u32,
                result_ptr: *mut u8,
                result_len: u32,
        ) -> i32;

        pub fn result_length() -> i32;
        pub fn fetch_result( dest: *mut u8);

        /// This extern marks an external import that we get from linking or
        /// environment. Usually this would be something pulled in from the Ethereum
        /// environement, but in this case we will use a later stage in the build
        /// process (cap9-build) to link in our own implementation of cap9_syscall
        /// to replace this import.
        ///
        /// A few notes on the API. All syscalls are delegate calls, therefore it
        /// returns an `i32` as with any other delegate call. This function here is
        /// the lowest level, therefore it's arguments are all the non-compulsory
        /// parts of a delgate call. That is, the signature of a delegate call is
        /// this:
        ///
        ///   dcall( gas: i64, address: *const u8, input_ptr: *const u8, input_len:
        ///      u32, result_ptr: *mut u8, result_len: u32, ) -> i32
        ///
        /// The `gas` and `address` are fixed by the system call specification,
        /// therefore we can only set the remaining parameters (`input_ptr`,
        /// `input_len`, `result_ptr`, and `result_len`);
        #[no_mangle]
        pub fn cap9_syscall_low(input_ptr: *const u8, input_len: u32, result_ptr: *mut u8, result_len: u32) -> i32;


    }

}

pub fn extcodesize(address: &Address) -> i32 {
    unsafe { external::extcodesize(address.as_ptr()) }
}

pub fn extcodecopy(address: &Address) -> pwasm_std::Vec<u8> {
    let len = unsafe { external::extcodesize(address.as_ptr()) };
    match len {
        0 => pwasm_std::Vec::new(),
        non_zero => {
            let mut data = pwasm_std::Vec::with_capacity(non_zero as usize);
            unsafe {
                data.set_len(non_zero as usize);
                external::extcodecopy(data.as_mut_ptr(), address.as_ptr());
            }
            data
        }
    }
}


pub fn actual_call_code(gas: u64, address: &Address, value: U256, input: &[u8], result: &mut [u8]) -> Result<(), Error> {
    let mut value_arr = [0u8; 32];
    value.to_big_endian(&mut value_arr);
    unsafe {
        if external::call_code(
            gas as i64,
            address.as_ptr(),
            value_arr.as_ptr(),
            input.as_ptr(),
            input.len() as u32,
            result.as_mut_ptr(), result.len() as u32
        ) == 0 {
            Ok(())
        } else {
            Err(Error)
        }
    }
}

/// Allocates and requests [`call`] return data (result)
pub fn result() -> pwasm_std::Vec<u8> {
    let len = unsafe { external::result_length() };

    match len {
        0 => pwasm_std::Vec::new(),
        non_zero => {
            let mut data = pwasm_std::Vec::with_capacity(non_zero as usize);
            unsafe {
                data.set_len(non_zero as usize);
                external::fetch_result(data.as_mut_ptr());
            }
            data
        }
    }
}

/// This function is the rough shape of a syscall. It's only purpose is to force
/// the inclusion/import of all the necessay Ethereum functions and prevent them
/// from being deadcode eliminated. As part of this, it is also necessary to
/// pass wasm-build "dummy_syscall" as a public api parameter, to ensure that it
/// is preserved.
///
/// TODO: this is something we would like to not have to do
#[no_mangle]
fn dummy_syscall() {
    pwasm_ethereum::gas_left();
    pwasm_ethereum::sender();
    unsafe {
        external::dcall(0,0 as *const u8, 0 as *const u8, 0, 0 as *mut u8, 0);
    }
}

/// This is to replace pwasm_ethereum::call_code, and uses [`cap9_syscall_low`]: fn.cap9_syscall_low.html
/// underneath instead of dcall. This is a slightly higher level abstraction
/// over cap9_syscall_low that uses Result types and the like. This is by no
/// means part of the spec, but more ergonomic Rust level library code. Actual
/// syscalls should be built on top of this.
///
/// # Errors
///
/// Returns [`Error`] in case syscall returns error
///
/// [`Error`]: struct.Error.html
pub fn cap9_syscall(input: &[u8], result: &mut [u8]) -> Result<(), Error> {
    unsafe {
        if external::cap9_syscall_low(
            input.as_ptr(),
            input.len() as u32,
            result.as_mut_ptr(),
            result.len() as u32
        ) == 0 {
            Ok(())
        } else {
            Err(Error)
        }
    }
}

pub fn write(cap_index: u8, key: &[u8; 32], value: &[u8; 32]) -> Result<(), Error> {
    let mut input = Vec::with_capacity(1 + 1 + 32 + 32);
    let syscall = SysCall {
        cap_index,
        action: SysCallAction::Write(WriteCall{key: key.into(), value: value.into()}),
    };
    syscall.serialize(&mut input).unwrap();
    cap9_syscall(&input, &mut Vec::new())
}

pub fn call(cap_index: u8, proc_id: SysCallProcedureKey, payload: Vec<u8>) -> Result<(), Error> {
    let mut input = Vec::new();
    let syscall = SysCall {
        cap_index,
        action: SysCallAction::Call(Call{proc_id: proc_id.0, payload: Payload(payload)}),
    };
    syscall.serialize(&mut input).unwrap();
    cap9_syscall(&input, &mut Vec::new())
}

pub fn log(cap_index: u8, topics: Vec<H256>, value: Vec<u8>) -> Result<(), Error> {
    let mut input: Vec<u8> = Vec::new();
    let syscall = SysCall {
        cap_index,
        action: SysCallAction::Log(LogCall{topics,value: Payload(value)}),
    };
    syscall.serialize(&mut input).unwrap();
    cap9_syscall(&input, &mut Vec::new())
}

pub fn reg(cap_index: u8, proc_id: SysCallProcedureKey, address: Address, cap_list: Vec<H256>) -> Result<(), Error> {
    let mut input = Vec::new();
    let u256_list: Vec<U256> = cap_list.iter().map(|x| x.into()).collect();
    let cap_list = proc_table::cap::NewCapList::from_u256_list(&u256_list).unwrap();
    let syscall = SysCall {
        cap_index,
        action: SysCallAction::Register(RegisterProc{proc_id: proc_id.0, address, cap_list}),
    };
    syscall.serialize(&mut input).unwrap();
    cap9_syscall(&input, &mut Vec::new())
}

pub fn delete(cap_index: u8, proc_id: SysCallProcedureKey) -> Result<(), Error> {
    let mut input = Vec::new();
    let syscall = SysCall {
        cap_index,
        action: SysCallAction::Delete(DeleteProc{proc_id: proc_id.0}),
    };
    syscall.serialize(&mut input).unwrap();
    cap9_syscall(&input, &mut Vec::new())
}

pub fn entry(cap_index: u8, proc_id: SysCallProcedureKey) -> Result<(), Error> {
    let mut input = Vec::new();
    let syscall = SysCall {
        cap_index,
        action: SysCallAction::SetEntry(SetEntry{proc_id: proc_id.0}),
    };
    syscall.serialize(&mut input).unwrap();
    cap9_syscall(&input, &mut Vec::new())
}
pub fn acc_call(cap_index: u8, address: Address, value: U256, payload: Vec<u8>) -> Result<(), Error> {
    let mut input = Vec::new();
    let syscall = SysCall {
        cap_index,
        action: SysCallAction::AccountCall(AccountCall{
            address,
            value,
            payload: Payload(payload),
        }),
    };
    syscall.serialize(&mut input).unwrap();
    cap9_syscall(&input, &mut Vec::new())
}

use core::marker::PhantomData;

/// This is a Cap9 map. The way Solidity maps and Cap9 caps work are not
/// compatible, as Cap9 uses contigous storage blocks in the caps. It is
/// _generally_ expected that caps will be used in such a way that they are
/// non-overlapping (although possibly shared). This means that key-size is
/// relevant in a map that we create. This map does not do any hashing, and if a
/// hashmap is desired that should be abstracted. This map associates one key of
/// a fixed size, with a number of 32-byte values in storage.
///
/// This structure makes sense when the keys are not sparse. That is: when the
/// number of used keys is within a few orders of maginitude of the number of
/// possible keys. This will only really occur when the keys are small. A good
/// example of this is an group permission map where each group id is only
/// 8-bits.
///
/// The values of this struct are intentionally private.
///
/// The value type must implement to/from Vec<H256>.
pub struct BigMap<T> {
    /// The size of the key in bits.
    key_bits: u8,
    /// The number of 32-byte values associated with each key in bits.
    ///
    ///  By "in bits" we mean the number of bits necessary to provide space for
    /// each value. This means that for a single 32-byte value, the number of
    /// bits required is 0. For a data type of 5 32-byte values the number of
    /// bits is 3, even though this could store up to
    ///
    value_bits: u8,
    /// The number of 32-byte values used for this data type. Must fit within
    /// value_bits.
    ///
    /// This is currently limited to a 32-bit number. Even though it could
    /// technically be 255 bits, value of that size would not be practical on
    /// Ethereum, and using a 32-bit number has more programming language
    /// support. Using 32-bit values also gives as very simple soundness
    /// properties that are useful at this stage (e.g. converts cleanly to f64).
    value_size: u32,
    /// The start location of the map.
    location: H256,
    /// The data type of the map
    data_type: PhantomData<T>,
}

impl<T: From<Vec<H256>> + Into<Vec<H256>>> BigMap<T> {

    // TODO: currently this accepts kernel space locations.
    pub fn new(key_bits: u8, value_size: u32, location: H256) -> Self {
        // This casts the log2 of the value size to u8. value_size is a u32, and
        // therefore the log2 of it will always fit inside a u8. See test:
        // log2_u32() for a demonstration of this.
        let value_bits = f64::from(value_size).log2().ceil() as u8;
        BigMap {
            key_bits,
            value_size,
            value_bits,
            location,
            data_type: PhantomData,
        }
    }

    pub fn key_bits(&self) -> u8 {
        self.key_bits
    }

    pub fn value_bits(&self) -> u8 {
        self.value_bits
    }

    pub fn value_size(&self) -> u32 {
        self.value_size
    }

    pub fn location(&self) -> H256 {
        self.location
    }

    // fn base_key(&self, key: u8) -> H256 {
    //     // base is simply a zeroed array in which we will store each piece of
    //     // info with the correct alignment.
    //     let mut key_mask [u8; 32] = [0; 32];
    //     // The key starts at 255 - value_bits - key bits.
    //     let key_start = 256 - self.value_bits - self.key_bits;
    //     let
    //     // key_mask[key_start..=(key_start+self.key_bits)].copy_from_slice();
    //     // key_mask[0..key_start].copy_from_slice();
    //     // let mut base = self.location.clone().to_fixed_bytes();
    //     // base[31] = key;
    //     // base.into()
    // }

    fn presence_key(&self, key: u8) -> H256 {
        // The presence_key is the storage key which indicates whether there is a
        // value associated with this key.
        let mut base = self.location.clone().to_fixed_bytes();
        base[30] = key;
        let mut presence_key = base.clone();
        presence_key[29] = 0;
        // For now this is fixed. The first 246-bits are determined by the
        // location. The next bit [246] is the presence/value bit. Then 8 bits [247,254]
        // are the key. The last bit [255] is for the value.
        presence_key.into()
    }

    pub fn present(&self, key: u8) -> bool {
        // If the value at the presence key is non-zero, then a value is
        // present.
        let present = pwasm_ethereum::read(&self.presence_key(key));
        (present[29] & 0b00000001) != 0
    }

    fn set_present(&self, key: u8) {
        // If the value at the presence key is non-zero, then a value is
        // present.
        let mut present = pwasm_ethereum::read(&self.presence_key(key));
        present[29] = present[29] | 0b00000001;
        pwasm_ethereum::write(&self.presence_key(key), &present);
    }

    pub fn get(&self, key: u8) -> Option<T> {
        // First question: Is there a value associated with this key?
        //
        // The presence_key is the storage key which indicates whether there is a
        // value associated with this key.
        let mut base = self.location.clone().to_fixed_bytes();
        base[30] = key;
        let mut presence_key = base.clone();
        presence_key[29] = 0;
        // For now this is fixed. The first 246-bits are determined by the
        // location. The next bit [246] is the presence/value bit. Then 8 bits [247,254]
        // are the key. The last bit [255] is for the value.
        let present = pwasm_ethereum::read(&presence_key.into());
        if present == [0; 32] {
            None
        } else {
            let mut vals: Vec<H256> = Vec::with_capacity(self.value_size as usize);
            for _ in 0..self.value_size {
                base[31] = base[31] + 1;
                vals.push(pwasm_ethereum::read(&base.into()).into());
            }
            Some(vals.into())
        }
    }

    pub fn insert(&mut self, key: u8, value: T) {
        let mut base = self.location.clone().to_fixed_bytes();
        base[30] = key;
        let mut presence_key = base.clone();
        presence_key[29] = 0;
        // For now this is fixed. The first 246-bits are determined by the
        // location. The next bit [246] is the presence/value bit. Then 8 bits [247,254]
        // are the key. The last bit [255] is for the value.
        self.set_present(key);
        let vals: Vec<H256> = value.into();
        for val in vals {
            base[31] = base[31] + 1;
            pwasm_ethereum::write(&base.into(), &val.into());
        }
    }
}

impl From<Vec<H256>> for SysCallProcedureKey {
        fn from(h: Vec<H256>) -> Self {
            h[0].into()
        }
    }

impl Into<Vec<H256>> for SysCallProcedureKey {
    fn into(self) -> Vec<H256> {
        let mut res = Vec::with_capacity(1);
        res.push(self.into());
        res
    }
}

#[cfg(test)]
mod test {
    use pwasm_abi::types::*;
    use super::*;

    #[derive(Debug,Clone,PartialEq)]
    struct ExampleData {
        key_v1: H256,
        key_v2: H256,
    }


    impl From<Vec<H256>> for ExampleData {
        fn from(h: Vec<H256>) -> Self {
            ExampleData {
                key_v1: h[0],
                key_v2: h[1],
            }
        }
    }

    impl Into<Vec<H256>> for ExampleData {
        fn into(self) -> Vec<H256> {
            let mut res = Vec::with_capacity(2);
            res.push(self.key_v1);
            res.push(self.key_v2);
            res
        }
    }

    #[test]
    fn new_big_map() {
        let location: H256 = [
            0xaa, 0xaa, 0xaa, 0xaa, 0xaa, 0xaa, 0xaa, 0xaa,
            0xaa, 0xaa, 0xaa, 0xaa, 0xaa, 0xaa, 0xaa, 0xaa,
            0xaa, 0xaa, 0xaa, 0xaa, 0xaa, 0xaa, 0xaa, 0xaa,
            0xaa, 0xaa, 0xaa, 0xaa, 0xaa, 0xaa, 0xaa, 0xaa,
        ].into();
        let mut map: BigMap<ExampleData> = BigMap::new(8, 5, location);
        assert_eq!(map.key_bits(), 8);
        assert_eq!(map.value_size(), 5);
        assert_eq!(map.value_bits(), 3);
        assert_eq!(map.location(), location);
        assert_eq!(map.get(1), None);
        let example = ExampleData {
            key_v1: H256::repeat_byte(0xdd),
            key_v2: H256::repeat_byte(0xee),
        };
        map.insert(1, example.clone());
        assert_eq!(map.get(1), Some(example));
    }

    /// A sanity check to show that log2 of any u32 is less than 255, and will
    /// therefore fit inside a u8, even when rounded up.
    #[test]
    fn log2_u32() {
        let value_bits = (f64::from(u32::max_value())).log2();
        assert!(value_bits < 255_f64);
    }
}
