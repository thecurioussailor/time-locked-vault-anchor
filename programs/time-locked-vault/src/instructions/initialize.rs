use anchor_lang::prelude::*;
use anchor_spl::token::{Mint, Token, TokenAccount};

use crate::{VaultError, VaultState, events::VaultInitialized};

pub const MIN_LOCK_DURATION: i64 = 60;
pub const MAX_LOCK_DURATION: i64 = 4 * 365 * 24 * 3600;

#[derive(Accounts)]
pub struct Initialize<'info> {
    #[account(mut)]
    pub owner: Signer<'info>,

    pub mint: Account<'info, Mint>,

    #[account(
        init,
        payer = owner,
        space = 8 + VaultState::INIT_SPACE,
        seeds = [b"vault_state", owner.key().as_ref(), mint.key().as_ref()],
        bump,
    )]
    pub vault_state: Account<'info, VaultState>,

    #[account(
        init,
        payer = owner,
        token::mint = mint,
        token::authority = vault_state,
        seeds = [b"vault", owner.key().as_ref(), mint.key().as_ref()],
        bump,
    )]
    pub vault_token_account: Account<'info, TokenAccount>,

    pub token_program: Program<'info, Token>,

    pub system_program: Program<'info, System>
}

impl<'info> Initialize<'info> {
    pub fn initialize(
        &mut self,
        bumps: &InitializeBumps,
        lock_duration: i64,
    ) -> Result<()> {
        
        require!(lock_duration >= MIN_LOCK_DURATION, VaultError::LockDurationTooShort);
        require!(lock_duration <= MAX_LOCK_DURATION, VaultError::LockDurationTooLong);
        
        let clock = Clock::get()?;
        let unlock_time = clock.unix_timestamp
            .checked_add(lock_duration)
            .ok_or(VaultError::Overflow)?;

        self.vault_state.set_inner( VaultState { 
            owner: self.owner.key(), 
            mint: self.mint.key(), 
            bump: bumps.vault_state, 
            vault_bump: bumps.vault_token_account, 
            lock_duration,
            unlock_time,
            total_deposited: 0, 
        });

        emit!(VaultInitialized {
            owner: self.owner.key(),
            mint: self.mint.key(),
            unlock_time,
            lock_duration,
        });
        
        Ok(())
    }
}