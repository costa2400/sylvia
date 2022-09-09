use cosmwasm_std::{Addr, CosmosMsg, Deps, DepsMut, Env, MessageInfo, Response};
use cw1::{CanExecuteResp, Cw1};

use crate::contract::Cw1WhitelistContract;
use crate::error::ContractError;

impl Cw1 for Cw1WhitelistContract<'_> {
    type Error = ContractError;

    fn execute(
        &self,
        ctx: (DepsMut, Env, MessageInfo),
        msgs: Vec<CosmosMsg>,
    ) -> Result<Response, ContractError> {
        let (deps, _, info) = ctx;

        if !self.is_admin(deps.as_ref(), &info.sender) {
            return Err(ContractError::Unauthorized {});
        }

        let resp = Response::new()
            .add_messages(msgs)
            .add_attribute("action", "execute");
        Ok(resp)
    }

    fn can_execute(
        &self,
        ctx: (Deps, Env),
        sender: String,
        _msg: CosmosMsg,
    ) -> Result<cw1::CanExecuteResp, Self::Error> {
        let (deps, _) = ctx;

        let resp = CanExecuteResp {
            can_execute: self.is_admin(deps, &Addr::unchecked(&sender)),
        };

        Ok(resp)
    }
}