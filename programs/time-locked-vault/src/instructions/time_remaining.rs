use anchor_lang::prelude::*;

use crate::VaultState;

#[derive(Accounts)]
pub struct TimeRemaining<'info> {
    /// CHECK: read-only account used only for PDA seed derivation; no data is read or written
    pub owner: UncheckedAccount<'info>,

    #[account(
        seeds = [b"vault_state", owner.key().as_ref(), vault_state.mint.as_ref()],
        bump = vault_state.bump,
    )]
    pub vault_state: Account<'info, VaultState>,

}

impl<'info> TimeRemaining<'info> {
    pub fn time_remaining(&self) -> Result<i64> {
        let clock = Clock::get()?;
        Ok((self.vault_state.unlock_time - clock.unix_timestamp).max(0))
    }
}