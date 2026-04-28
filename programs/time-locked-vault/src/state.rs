use anchor_lang::prelude::*;

#[account]
#[derive(InitSpace)]
pub struct VaultState {
    pub owner: Pubkey,
    pub mint: Pubkey,
    pub bump: u8,
    pub vault_bump: u8,
    pub lock_duration: i64,
    pub unlock_time: i64,
    pub total_deposited: u64,
}