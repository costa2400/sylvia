use crate::error::ContractError;
use crate::responses::AdminListResponse;
use cosmwasm_std::{Addr, Deps, DepsMut, Empty, Env, MessageInfo, Order, Response, StdResult};

use cw2::set_contract_version;
use cw_storage_plus::{Item, Map};
use sylvia::contract;

const CONTRACT_NAME: &str = env!("CARGO_PKG_NAME");
const CONTRACT_VERSION: &str = env!("CARGO_PKG_VERSION");

pub struct Cw1WhitelistContract<'a> {
    pub(crate) admins: Map<'static, &'a Addr, Empty>,
    pub(crate) mutable: Item<'static, bool>,
}

#[contract]
#[messages(cw1 as Cw1)]
impl Cw1WhitelistContract<'_> {
    pub const fn new() -> Self {
        Self {
            admins: Map::new("admins"),
            mutable: Item::new("mutable"),
        }
    }

    #[msg(instantiate)]
    pub fn instantiate(
        &self,
        ctx: (DepsMut, Env, MessageInfo),
        admins: Vec<String>,
        mutable: bool,
    ) -> Result<Response, ContractError> {
        let (deps, _, _) = ctx;
        set_contract_version(deps.storage, CONTRACT_NAME, CONTRACT_VERSION)?;

        for admin in admins {
            let admin = deps.api.addr_validate(&admin)?;
            self.admins.save(deps.storage, &admin, &Empty {})?;
        }

        self.mutable.save(deps.storage, &mutable)?;

        Ok(Response::new())
    }

    #[msg(exec)]
    pub fn freeze(&self, ctx: (DepsMut, Env, MessageInfo)) -> Result<Response, ContractError> {
        let (deps, _, info) = ctx;

        if !self.is_admin(deps.as_ref(), &info.sender) {
            return Err(ContractError::Unauthorized {});
        }

        self.mutable.save(deps.storage, &false)?;

        let resp = Response::new().add_attribute("action", "freeze");
        Ok(resp)
    }

    #[msg(exec)]
    pub fn update_admins(
        &self,
        ctx: (DepsMut, Env, MessageInfo),
        mut admins: Vec<String>,
    ) -> Result<Response, ContractError> {
        let (deps, _, info) = ctx;

        if !self.is_admin(deps.as_ref(), &info.sender) {
            return Err(ContractError::Unauthorized {});
        }

        if !self.mutable.load(deps.storage)? {
            return Err(ContractError::ContractFrozen {});
        }

        admins.sort_unstable();
        let mut low_idx = 0;

        let to_remove: Vec<_> = self
            .admins
            .keys(deps.storage, None, None, Order::Ascending)
            .filter(|addr| {
                // This is a bit of optimization basing on the fact that both `admins` and queried
                // keys range are sorted. Binary search would always return the index which is at
                // most as big as searched item, so for next item there is no point in looking at
                // lower indices. On the other hand - if we reached and of the sequence, we want to
                // remove all following keys.
                addr.as_ref()
                    .map(|addr| {
                        if low_idx >= admins.len() {
                            return true;
                        }

                        match admins[low_idx..].binary_search(&addr.into()) {
                            Ok(idx) => {
                                low_idx = idx;
                                false
                            }
                            Err(idx) => {
                                low_idx = idx;
                                true
                            }
                        }
                    })
                    .unwrap_or(true)
            })
            .collect::<Result<_, _>>()?;

        for addr in to_remove {
            self.admins.remove(deps.storage, &addr);
        }

        for admin in admins {
            let admin = deps.api.addr_validate(&admin)?;
            self.admins.save(deps.storage, &admin, &Empty {})?;
        }

        let resp = Response::new().add_attribute("action", "update_admins");
        Ok(resp)
    }

    #[msg(query)]
    pub fn admin_list(&self, ctx: (Deps, Env)) -> StdResult<AdminListResponse> {
        let (deps, _) = ctx;

        let admins: Result<_, _> = self
            .admins
            .keys(deps.storage, None, None, Order::Ascending)
            .map(|addr| addr.map(String::from))
            .collect();

        Ok(AdminListResponse {
            admins: admins?,
            mutable: self.mutable.load(deps.storage)?,
        })
    }

    pub fn is_admin(&self, deps: Deps, addr: &Addr) -> bool {
        self.admins.has(deps.storage, addr)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cosmwasm_std::testing::{mock_dependencies, mock_env, mock_info};
    use cosmwasm_std::{coin, coins, to_binary, BankMsg, CosmosMsg, StakingMsg, SubMsg, WasmMsg};
    use cw1::Cw1;

    #[test]
    fn instantiate_and_modify_config() {
        let mut deps = mock_dependencies();

        let alice = "alice";
        let bob = "bob";
        let carl = "carl";

        let anyone = "anyone";

        let contract = Cw1WhitelistContract::new();

        // instantiate the contract
        let info = mock_info(anyone, &[]);
        contract
            .instantiate(
                (deps.as_mut(), mock_env(), info),
                vec![alice.to_string(), bob.to_string(), carl.to_string()],
                true,
            )
            .unwrap();

        // ensure expected config
        let expected = AdminListResponse {
            admins: vec![alice.to_string(), bob.to_string(), carl.to_string()],
            mutable: true,
        };
        assert_eq!(
            contract.admin_list((deps.as_ref(), mock_env())).unwrap(),
            expected
        );

        // anyone cannot modify the contract
        let info = mock_info(anyone, &[]);
        let err = contract
            .update_admins((deps.as_mut(), mock_env(), info), vec![anyone.to_string()])
            .unwrap_err();
        assert_eq!(err, ContractError::Unauthorized {});

        // but alice can kick out carl
        let info = mock_info(alice, &[]);
        contract
            .update_admins(
                (deps.as_mut(), mock_env(), info),
                vec![alice.to_string(), bob.to_string()],
            )
            .unwrap();

        // ensure expected config
        let expected = AdminListResponse {
            admins: vec![alice.to_string(), bob.to_string()],
            mutable: true,
        };
        assert_eq!(
            contract.admin_list((deps.as_ref(), mock_env())).unwrap(),
            expected
        );

        // carl cannot freeze it
        let info = mock_info(carl, &[]);
        let err = contract
            .freeze((deps.as_mut(), mock_env(), info))
            .unwrap_err();
        assert_eq!(err, ContractError::Unauthorized {});

        // but bob can
        let info = mock_info(bob, &[]);
        contract.freeze((deps.as_mut(), mock_env(), info)).unwrap();
        let expected = AdminListResponse {
            admins: vec![alice.to_string(), bob.to_string()],
            mutable: false,
        };
        assert_eq!(
            contract.admin_list((deps.as_ref(), mock_env())).unwrap(),
            expected
        );

        // and now alice cannot change it again
        let info = mock_info(alice, &[]);
        let err = contract
            .update_admins((deps.as_mut(), mock_env(), info), vec![alice.to_string()])
            .unwrap_err();
        assert_eq!(err, ContractError::ContractFrozen {});
    }

    #[test]
    fn execute_messages_has_proper_permissions() {
        let mut deps = mock_dependencies();

        let alice = "alice";
        let bob = "bob";
        let carl = "carl";

        let contract = Cw1WhitelistContract::new();

        // instantiate the contract
        let info = mock_info(bob, &[]);
        contract
            .instantiate(
                (deps.as_mut(), mock_env(), info),
                vec![alice.to_string(), carl.to_string()],
                false,
            )
            .unwrap();

        let freeze = ImplExecMsg::Freeze {};
        let msgs = vec![
            BankMsg::Send {
                to_address: bob.to_string(),
                amount: coins(10000, "DAI"),
            }
            .into(),
            WasmMsg::Execute {
                contract_addr: "some contract".into(),
                msg: to_binary(&freeze).unwrap(),
                funds: vec![],
            }
            .into(),
        ];

        // bob cannot execute them
        let info = mock_info(bob, &[]);
        let err = contract
            .execute((deps.as_mut(), mock_env(), info), msgs.clone())
            .unwrap_err();
        assert_eq!(err, ContractError::Unauthorized {});

        // but carl can
        let info = mock_info(carl, &[]);
        let res = contract
            .execute((deps.as_mut(), mock_env(), info), msgs.clone())
            .unwrap();
        assert_eq!(
            res.messages,
            msgs.into_iter().map(SubMsg::new).collect::<Vec<_>>()
        );
        assert_eq!(res.attributes, [("action", "execute")]);
    }

    #[test]
    fn can_execute_query_works() {
        let mut deps = mock_dependencies();

        let alice = "alice";
        let bob = "bob";

        let anyone = "anyone";

        let contract = Cw1WhitelistContract::new();

        // instantiate the contract
        let info = mock_info(anyone, &[]);
        contract
            .instantiate(
                (deps.as_mut(), mock_env(), info),
                vec![alice.to_string(), bob.to_string()],
                false,
            )
            .unwrap();

        // let us make some queries... different msg types by owner and by other
        let send_msg = CosmosMsg::Bank(BankMsg::Send {
            to_address: anyone.to_string(),
            amount: coins(12345, "ushell"),
        });
        let staking_msg = CosmosMsg::Staking(StakingMsg::Delegate {
            validator: anyone.to_string(),
            amount: coin(70000, "ureef"),
        });

        // owner can send
        let res = contract
            .can_execute(
                (deps.as_ref(), mock_env()),
                alice.to_string(),
                send_msg.clone(),
            )
            .unwrap();
        assert!(res.can_execute);

        // owner can stake
        let res = contract
            .can_execute(
                (deps.as_ref(), mock_env()),
                bob.to_string(),
                staking_msg.clone(),
            )
            .unwrap();
        assert!(res.can_execute);

        // anyone cannot send
        let res = contract
            .can_execute((deps.as_ref(), mock_env()), anyone.to_string(), send_msg)
            .unwrap();
        assert!(!res.can_execute);

        // anyone cannot stake
        let res = contract
            .can_execute((deps.as_ref(), mock_env()), anyone.to_string(), staking_msg)
            .unwrap();
        assert!(!res.can_execute);
    }

    mod msgs {
        use cosmwasm_std::{from_binary, from_slice, to_binary, BankMsg};

        use crate::contract::{ExecMsg, ImplExecMsg, ImplQueryMsg, QueryMsg};

        #[test]
        fn freeze() {
            let original = ImplExecMsg::Freeze {};
            let serialized = to_binary(&original).unwrap();
            let deserialized = from_binary(&serialized).unwrap();

            assert_eq!(ExecMsg::Cw1WhitelistContract(original), deserialized);

            let json = br#"{
                "freeze": {}
            }"#;
            let deserialized = from_slice(json).unwrap();

            assert_eq!(
                ExecMsg::Cw1WhitelistContract(ImplExecMsg::Freeze {}),
                deserialized
            );
        }

        #[test]
        fn update_admins() {
            let original = ImplExecMsg::UpdateAdmins {
                admins: vec!["admin1".to_owned(), "admin2".to_owned()],
            };
            let serialized = to_binary(&original).unwrap();
            let deserialized = from_binary(&serialized).unwrap();

            assert_eq!(ExecMsg::Cw1WhitelistContract(original), deserialized);

            let json = br#"{
                "update_admins": {
                    "admins": ["admin1", "admin3"]
                }
            }"#;
            let deserialized = from_slice(json).unwrap();

            assert_eq!(
                ExecMsg::Cw1WhitelistContract(ImplExecMsg::UpdateAdmins {
                    admins: vec!["admin1".to_owned(), "admin3".to_owned()]
                }),
                deserialized
            );
        }

        #[test]
        fn admin_list() {
            let original = ImplQueryMsg::AdminList {};
            let serialized = to_binary(&original).unwrap();
            let deserialized = from_binary(&serialized).unwrap();

            assert_eq!(QueryMsg::Cw1WhitelistContract(original), deserialized);

            let json = br#"{
                "admin_list": {}
            }"#;
            let deserialized = from_slice(json).unwrap();

            assert_eq!(
                QueryMsg::Cw1WhitelistContract(ImplQueryMsg::AdminList {}),
                deserialized
            );
        }

        #[test]
        fn execute() {
            let original = cw1::ExecMsg::Execute {
                msgs: vec![BankMsg::Send {
                    to_address: "admin1".to_owned(),
                    amount: vec![],
                }
                .into()],
            };
            let serialized = to_binary(&original).unwrap();
            let deserialized = from_binary(&serialized).unwrap();
            assert_eq!(ExecMsg::Cw1(original), deserialized);
        }

        #[test]
        fn can_execute() {
            let original = cw1::QueryMsg::CanExecute {
                sender: "admin".to_owned(),
                msg: BankMsg::Send {
                    to_address: "admin1".to_owned(),
                    amount: vec![],
                }
                .into(),
            };
            let serialized = to_binary(&original).unwrap();
            let deserialized = from_binary(&serialized).unwrap();
            assert_eq!(QueryMsg::Cw1(original), deserialized);
        }
    }
}