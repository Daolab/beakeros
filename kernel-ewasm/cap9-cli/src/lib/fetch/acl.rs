use crate::utils::{from_common_address, from_common_u256};
use cap9_std::proc_table::cap::*;
use pwasm_abi;
use web3::types::{Address, H256, U256};
use std::fs::File;
use web3::contract::tokens::Tokenize;
use web3::contract::Options;
use web3::futures::Future;
use web3::Transport;

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use super::kernel::*;
use super::map::*;
use crate::default_procedures;
use crate::project::*;

/// As with [DeployKernel] but with a standard ACL.
pub struct DeployedKernelWithACL<'a, T: Transport> {
    pub kernel: DeployedKernel<'a, T>,
}

impl<'a, T: Transport> DeployedKernelWithACL<'a, T> {
    pub fn new(kernel: DeployedKernel<'a, T>) -> Self {
        DeployedKernelWithACL { kernel: kernel }
    }

    pub fn groups(&self) -> HashMap<u8, Group> {
        // Currently we assume the group map is at cap index 1
        let groups: LocalEnumerableMap<_, u8, cap9_std::SysCallProcedureKey> =
            LocalEnumerableMap::from(&self.kernel, 1).expect("could not create group map");
        // Currently we assume the users map is at cap index 0
        let users: LocalEnumerableMap<_, pwasm_abi::types::Address, u8> =
            LocalEnumerableMap::from(&self.kernel, 0).expect("could not create user map");
        let mut group_map: HashMap<u8, Group> = HashMap::new();
        for (k, v) in groups.iter() {
            group_map.insert(
                k,
                Group {
                    id: k,
                    procedure_key: v,
                    users: HashSet::new(),
                },
            );
        }
        for (k, v) in users.iter() {
            let group = group_map
                .get_mut(&v)
                .expect(format!("no such group exists: {}", v).as_str());
            group.users.insert(from_common_address(k));
        }
        group_map
    }

    pub fn users(&self) -> HashMap<Address, u8> {
        // Currently we assume the users map is at cap index 0
        let users: LocalEnumerableMap<_, pwasm_abi::types::Address, u8> =
            LocalEnumerableMap::from(&self.kernel, 0).expect("could not create user map");
        let mut users_map = HashMap::new();
        for (k, v) in users.iter() {
            users_map.insert(from_common_address(k), v);
        }
        users_map
    }

    pub fn get_group_proc(&self, address: &Address) -> cap9_std::SysCallProcedureKey {
        let users_map = self.users();
        let user_group = users_map.get(address).unwrap();
        let group_map = self.groups();
        let group = group_map.get(user_group).unwrap();
        group.procedure_key.clone()
    }

    pub fn new_group(
        &self,
        group_number: u8,
        proc_name: String,
        group_proc: ProcSpec,
    ) -> Result<(), ProjectDeploymentError> {
        let proc_key = crate::utils::string_to_proc_key(proc_name);
        let cap_index = 0;
        // Deploy the contract to the blockchain.
        let contract = group_proc.deploy(&self.kernel.conn, ()).unwrap();

        let cap_file = File::open(group_proc.cap_path).expect("could not open file");
        let crate::fetch::procedure::SerialNewCapList(caps) =
            serde_json::from_reader(cap_file).unwrap();

        let existing_caps: Capabilities = self
            .kernel
            .procedure(self.admin_proc_key().expect("no admin key"))
            .expect("no admin proc")
            .caps
            .into();
        let cap_test = caps.check_subset_of(existing_caps);
        if cap_test.len() != 0 {
            panic!("invalid caps: {:?}", cap_test);
        }
        let cap_list: Vec<U256> = caps
            .to_u256_list()
            .into_iter()
            .map(from_common_u256)
            .collect();
        // let cap_list: Vec<U256> = vec![];

        let _proxied_admin_contract = web3::contract::Contract::from_json(
            self.kernel.conn.web3.eth(),
            self.kernel.address(),
            group_proc.contract_spec.abi().as_slice(),
        )
        .map_err(|err| ProjectDeploymentError::ProxiedProcedureError {
            err: format!("{:?}", err),
        })?;

        let encoded_proc_key: U256 = crate::utils::proc_key_to_32_bytes(&proc_key).into();

        let params = (cap_index, encoded_proc_key, contract.address(), cap_list);
        // Register the procedure
        let file: &[u8] = default_procedures::ACL_ADMIN.abi();
        let admin_abi = ethabi::Contract::load(file).expect("no ABI");
        let message: Vec<u8> = admin_abi
            .function("regProc")
            .and_then(|function| function.encode_input(params.into_tokens().as_slice()))
            .expect("message encoding failed");
        let proxied_entry_contract = web3::contract::Contract::from_json(
            self.kernel.conn.web3.eth(),
            self.kernel.address(),
            default_procedures::ACL_ENTRY.abi(),
        )
        .map_err(|err| ProjectDeploymentError::ProxiedProcedureError {
            err: format!("{:?}", err),
        })?;

        let res = proxied_entry_contract
            .call(
                "proxy",
                (message,),
                self.kernel.conn.sender,
                Options::with(|opts| {
                    opts.gas = Some(550_621_180.into());
                }),
            )
            .wait()
            .expect("proxy");
        let reg_receipt = &self
            .kernel
            .conn
            .web3
            .eth()
            .transaction_receipt(res)
            .wait()
            .expect("reg receipt")
            .unwrap();
        if reg_receipt.status != Some(web3::types::U64::one()) {
            panic!("ACL register proc failed! {:?}", reg_receipt);
        }
        // use the kernel address as the test account
        let test_account = self.kernel.address().clone();

        let new_group_params = (test_account, U256::from(group_number));
        let new_group_message: Vec<u8> = admin_abi
            .function("set_account_group")
            .and_then(|function| function.encode_input(new_group_params.into_tokens().as_slice()))
            .expect("message encoding failed");
        let res = proxied_entry_contract
            .call(
                "proxy",
                (new_group_message,),
                self.kernel.conn.sender,
                Options::with(|opts| {
                    opts.gas = Some(550_621_180.into());
                }),
            )
            .wait()
            .expect("proxy");
        let new_group_receipt = &self
            .kernel
            .conn
            .web3
            .eth()
            .transaction_receipt(res)
            .wait()
            .expect("new_group receipt")
            .unwrap();
        if new_group_receipt.status != Some(web3::types::U64::one()) {
            panic!("ACL set group failed!");
        }

        let new_group_params = (U256::from(5), encoded_proc_key);
        let new_group_message: Vec<u8> = admin_abi
            .function("set_group_procedure")
            .and_then(|function| function.encode_input(new_group_params.into_tokens().as_slice()))
            .expect("message encoding failed");
        let res = proxied_entry_contract
            .call(
                "proxy",
                (new_group_message,),
                self.kernel.conn.sender,
                Options::with(|opts| {
                    opts.gas = Some(550_621_180.into());
                }),
            )
            .wait()
            .expect("proxy");
        let new_group_receipt = &self
            .kernel
            .conn
            .web3
            .eth()
            .transaction_receipt(res)
            .wait()
            .expect("new_group receipt")
            .unwrap();
        if new_group_receipt.status != Some(web3::types::U64::one()) {
            panic!("ACL register proc failed!");
        }
        Ok(())
    }

    pub fn group_key(&self, index: u8) -> Option<cap9_std::SysCallProcedureKey> {
        // Currently we assume the group map is at cap index 1
        let groups: LocalEnumerableMap<_, u8, cap9_std::SysCallProcedureKey> =
            LocalEnumerableMap::from(&self.kernel, 1).expect("could not create group map");
        groups.get(index)
    }

    pub fn admin_proc_key(&self) -> Option<cap9_std::SysCallProcedureKey> {
        self.group_key(1_u8)
    }

    // pub fn group(&self, index: u8) -> Option<Group> {
    //     // Currently we assume the group map is at cap index 1
    //     let groups: LocalEnumerableMap<_, u8, cap9_std::SysCallProcedureKey> = LocalEnumerableMap::from(&self.kernel, 1).expect("could not create group map");
    //     groups.get(index)
    // }

    /// Simply take a contract, deploy it, and register it as a procedure.
    pub fn deploy_procedure(
        &mut self,
        proc_name: String,
        proc_spec: ProcSpec,
    ) -> Result<(), ProjectDeploymentError> {
        let proc_key = crate::utils::string_to_proc_key(proc_name);

        let cap_file = File::open(&proc_spec.cap_path).expect("could not open file");
        let crate::fetch::procedure::SerialNewCapList(caps) =
            serde_json::from_reader(cap_file).unwrap();

        let cap_index = 0;
        let contract = proc_spec.deploy(&self.kernel.conn, ()).unwrap();
        let existing_caps: Capabilities = self
            .kernel
            .procedure(self.admin_proc_key().expect("no admin key"))
            .expect("no admin proc")
            .caps
            .into();
        let cap_test = caps.check_subset_of(existing_caps);
        if cap_test.len() != 0 {
            panic!("invalid caps: {:?}", cap_test);
        }
        let cap_list: Vec<U256> = caps
            .to_u256_list()
            .into_iter()
            .map(from_common_u256)
            .collect();
        let _proxied_admin_contract = web3::contract::Contract::from_json(
            self.kernel.conn.web3.eth(),
            self.kernel.address(),
            proc_spec.contract_spec.abi().as_slice(),
        )
        .map_err(|err| ProjectDeploymentError::ProxiedProcedureError {
            err: format!("{:?}", err),
        })?;

        let encoded_proc_key: U256 = crate::utils::proc_key_to_32_bytes(&proc_key).into();

        let params = (cap_index, encoded_proc_key, contract.address(), cap_list);
        // Register the procedure
        let file: &[u8] = default_procedures::ACL_ADMIN.abi();
        let admin_abi = ethabi::Contract::load(file).expect("no ABI");
        let message: Vec<u8> = admin_abi
            .function("regProc")
            .and_then(|function| function.encode_input(params.into_tokens().as_slice()))
            .expect("message encoding failed");
        let proxied_entry_contract = web3::contract::Contract::from_json(
            self.kernel.conn.web3.eth(),
            self.kernel.address(),
            default_procedures::ACL_ENTRY.abi(),
        )
        .map_err(|err| ProjectDeploymentError::ProxiedProcedureError {
            err: format!("{:?}", err),
        })?;

        let res = proxied_entry_contract
            .call(
                "proxy",
                (message,),
                self.kernel.conn.sender,
                Options::with(|opts| {
                    opts.gas = Some(550_621_180.into());
                }),
            )
            .wait()
            .expect("proxy");
        let reg_receipt = &self
            .kernel
            .conn
            .web3
            .eth()
            .transaction_receipt(res)
            .wait()
            .expect("reg receipt")
            .unwrap();
        if reg_receipt.status != Some(web3::types::U64::one()) {
            panic!("ACL register proc failed!");
        }
        // Add the ABI to the status file.
        let status_file: &mut StatusFile = (&mut self.kernel.local_project)
            .status_file_mut()
            .as_mut()
            .unwrap();
        status_file.add_abi(
            contract.address(),
            PathBuf::from(proc_spec.contract_spec.abi_path),
        );
        // Rewrite the status file to disk.
        self.kernel.local_project.write_status_file();
        Ok(())
    }

    /// Unregister an existing procedure.
    pub fn delete_procedure(&mut self, proc_name: String) -> Result<(), ProjectDeploymentError> {
        let proc_key = crate::utils::string_to_proc_key(proc_name);

        let cap_index = 0;
        let _proxied_admin_contract = web3::contract::Contract::from_json(
            self.kernel.conn.web3.eth(),
            self.kernel.address(),
            default_procedures::ACL_ADMIN.abi(),
        )
        .map_err(|err| ProjectDeploymentError::ProxiedProcedureError {
            err: format!("{:?}", err),
        })?;

        let encoded_proc_key: U256 = crate::utils::proc_key_to_32_bytes(&proc_key).into();

        let params = (cap_index, encoded_proc_key);
        self.kernel
            .conn
            .web3
            .personal()
            .unlock_account(self.kernel.conn.sender, "user", None)
            .wait()
            .unwrap();
        // Register the procedure
        let file: &[u8] = default_procedures::ACL_ADMIN.abi();
        let admin_abi = ethabi::Contract::load(file).expect("no ABI");
        let message: Vec<u8> = admin_abi
            .function("delProc")
            .and_then(|function| function.encode_input(params.into_tokens().as_slice()))
            .expect("message encoding failed");
        let proxied_entry_contract = web3::contract::Contract::from_json(
            self.kernel.conn.web3.eth(),
            self.kernel.address(),
            default_procedures::ACL_ENTRY.abi(),
        )
        .map_err(|err| ProjectDeploymentError::ProxiedProcedureError {
            err: format!("{:?}", err),
        })?;

        let res = proxied_entry_contract
            .call(
                "proxy",
                (message,),
                self.kernel.conn.sender,
                Options::with(|opts| {
                    opts.gas = Some(550_621_180.into());
                }),
            )
            .wait()
            .expect("proxy");
        let reg_receipt = &self
            .kernel
            .conn
            .web3
            .eth()
            .transaction_receipt(res)
            .wait()
            .expect("reg receipt")
            .unwrap();
        if reg_receipt.status != Some(web3::types::U64::one()) {
            panic!("ACL register proc failed!");
        }
        Ok(())
    }

    // pub fn abis(&self, proc_key: cap9_std::SysCallProcedureKey) -> Option<Procedure> {
    //     let status_file = self.kernel.local_project.status_file()?;
    //     let procs = self.procedures();
    //     for procedure in procs {
    //         if procedure.key == proc_key.0 {
    //             return Some(procedure);
    //         }
    //     }
    //     None
    // }

    pub fn call(
        &self,
        function_name: &str,
        params: &[ethabi::Token],
    ) -> web3::types::TransactionReceipt {
        let file: &[u8] = default_procedures::ACL_ADMIN.abi();
        let admin_abi = ethabi::Contract::load(file).expect("no ABI");
        let proxied_entry_contract = web3::contract::Contract::from_json(
            self.kernel.conn.web3.eth(),
            self.kernel.address(),
            default_procedures::ACL_ENTRY.abi(),
        )
        .map_err(|err| ProjectDeploymentError::ProxiedProcedureError {
            err: format!("{:?}", err),
        })
        .unwrap();
        let message: Vec<u8> = admin_abi
            .function(function_name)
            .and_then(|function| function.encode_input(params))
            .expect("message encoding failed");
        self.kernel
            .conn
            .web3
            .personal()
            .unlock_account(self.kernel.conn.sender, "user", None)
            .wait()
            .unwrap();
        let res = proxied_entry_contract
            .call(
                "proxy",
                (message,),
                self.kernel.conn.sender,
                Options::with(|opts| {
                    opts.gas = Some(550_621_180.into());
                }),
            )
            .wait()
            .expect("proxy");
        let reg_receipt = &self
            .kernel
            .conn
            .web3
            .eth()
            .transaction_receipt(res)
            .wait()
            .expect("reg receipt")
            .unwrap();
        if reg_receipt.status != Some(web3::types::U64::one()) {
            panic!("Call to {} failed!", function_name);
        }
        reg_receipt.clone()
    }

    pub fn call_any(
        &self,
        proc_key: cap9_std::SysCallProcedureKey,
        function_name: &str,
        params: &[ethabi::Token],
    ) -> web3::types::TransactionReceipt {
        println!("Using Call-Any");
        let file: &[u8] = default_procedures::ACL_ADMIN.abi();
        let admin_abi = ethabi::Contract::load(file).expect("no ABI");
        let proxied_entry_contract = web3::contract::Contract::from_json(
            self.kernel.conn.web3.eth(),
            self.kernel.address(),
            default_procedures::ACL_ENTRY.abi(),
        )
        .map_err(|err| ProjectDeploymentError::ProxiedProcedureError {
            err: format!("{:?}", err),
        })
        .unwrap();

        let proc_abi = {
            let procedure = self.kernel.procedure(proc_key.clone()).unwrap();
            let status_file: &StatusFile = self
                .kernel
                .local_project
                .status_file()
                .as_ref()
                .expect("could not get status file");
            let abi_path = status_file.abis.get(&procedure.address).expect("could not find ABI");
            // println!("ABI Path: {:?}", abi_path);
            let abi_file = File::open(abi_path).unwrap();
            let abi = ethabi::Contract::load(abi_file).unwrap();
            // println!("ABI: {:?}", abi);
            abi
        };

        let final_message: Vec<u8> = proc_abi
            .function(function_name)
            .and_then(|function| function.encode_input(params))
            .expect("message encoding failed");

        let k1: pwasm_abi::types::H256 = proc_key.into();
        let k: pwasm_abi::types::U256 = k1.into();
        let admin_params = vec![ethabi::Token::Uint(from_common_u256(k)), ethabi::Token::Bytes(final_message)];
        let message: Vec<u8> = admin_abi
            .function("call_any")
            .and_then(|function| function.encode_input(admin_params.as_slice()))
            .expect("message encoding failed");
        self.kernel
            .conn
            .web3
            .personal()
            .unlock_account(self.kernel.conn.sender, "user", None)
            .wait()
            .unwrap();
        let res = proxied_entry_contract
            .call(
                "proxy",
                (message,),
                self.kernel.conn.sender,
                Options::with(|opts| {
                    opts.gas = Some(550_621_180.into());
                }),
            )
            .wait()
            .expect("proxy");
        let reg_receipt = &self
            .kernel
            .conn
            .web3
            .eth()
            .transaction_receipt(res)
            .wait()
            .expect("reg receipt")
            .unwrap();
        if reg_receipt.status != Some(web3::types::U64::one()) {
            panic!("Call to {} failed!", function_name);
        }
        reg_receipt.clone()
    }

    pub fn query_any(
        &self,
        proc_key: cap9_std::SysCallProcedureKey,
        function_name: &str,
        params: &[ethabi::Token],
    ) -> ethabi::Result<Vec<ethabi::Token>> {
        println!("Using Query-Any");
        let file: &[u8] = default_procedures::ACL_ADMIN.abi();
        let admin_abi = ethabi::Contract::load(file).expect("no ABI");
        let proxied_entry_contract = web3::contract::Contract::from_json(
            self.kernel.conn.web3.eth(),
            self.kernel.address(),
            default_procedures::ACL_ENTRY.abi(),
        )
        .map_err(|err| ProjectDeploymentError::ProxiedProcedureError {
            err: format!("{:?}", err),
        })
        .unwrap();

        let proc_abi = {
            let procedure = self.kernel.procedure(proc_key.clone()).unwrap();
            let status_file: &StatusFile = self
                .kernel
                .local_project
                .status_file()
                .as_ref()
                .expect("could not get status file");
            let abi_path = status_file.abis.get(&procedure.address).expect("could not find ABI");
            let abi_file = File::open(abi_path).unwrap();
            let abi = ethabi::Contract::load(abi_file).unwrap();
            abi
        };

        let final_message: Vec<u8> = proc_abi
            .function(function_name)
            .and_then(|function| function.encode_input(params))
            .expect("message encoding failed");

        let k1: pwasm_abi::types::H256 = proc_key.into();
        let k: pwasm_abi::types::U256 = k1.into();
        let admin_params = vec![ethabi::Token::Uint(from_common_u256(k)), ethabi::Token::Bytes(final_message)];
        let message: Vec<u8> = admin_abi
            .function("call_any")
            .and_then(|function| function.encode_input(admin_params.as_slice()))
            .expect("message encoding failed");
        self.kernel
            .conn
            .web3
            .personal()
            .unlock_account(self.kernel.conn.sender, "user", None)
            .wait()
            .unwrap();
        let res = proxied_entry_contract
            .query_tokens(
                "proxy",
                (message,),
                self.kernel.conn.sender,
                proc_abi.function(function_name).unwrap(),
                Options::with(|opts| {
                    opts.gas = Some(550_621_180.into());
                }),
                None,
            )
            .map_err(|_| ethabi::Error::from_kind(ethabi::ErrorKind::Msg("bad query".to_string())))
            .wait();
        res
    }

    pub fn query(
        &self,
        function_name: &str,
        params: &[ethabi::Token],
    ) -> ethabi::Result<Vec<ethabi::Token>> {
        let file: &[u8] = default_procedures::ACL_ADMIN.abi();
        let admin_abi = ethabi::Contract::load(file).expect("no ABI");
        let proxied_entry_contract = web3::contract::Contract::from_json(
            self.kernel.conn.web3.eth(),
            self.kernel.address(),
            default_procedures::ACL_ENTRY.abi(),
        )
        .map_err(|err| ProjectDeploymentError::ProxiedProcedureError {
            err: format!("{:?}", err),
        })
        .unwrap();
        let message: Vec<u8> = admin_abi
            .function(function_name)
            .and_then(|function| function.encode_input(params))
            .expect("message encoding failed");
        self.kernel
            .conn
            .web3
            .personal()
            .unlock_account(self.kernel.conn.sender, "user", None)
            .wait()
            .unwrap();
        let res: ethabi::Result<Vec<ethabi::Token>> = proxied_entry_contract
            .query_tokens(
                "proxy",
                (message,),
                self.kernel.conn.sender,
                admin_abi.function(function_name).unwrap(),
                Options::with(|opts| {
                    opts.gas = Some(550_621_180.into());
                }),
                None,
            )
            .map_err(|_| ethabi::Error::from_kind(ethabi::ErrorKind::Msg("bad query".to_string())))
            .wait();
        res
    }
}

#[derive(Clone, Debug)]
pub struct Group {
    pub id: u8,
    pub procedure_key: cap9_std::SysCallProcedureKey,
    pub users: HashSet<Address>,
}
