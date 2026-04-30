use anchor_lang::prelude::*;
use anchor_spl::token::{self, Token, TokenAccount, Transfer};

use crate::{VaultError, VaultState, events::Withdrawn};

#[derive(Accounts)]
pub struct Withdraw<'info> {
    #[account(mut)]
    pub owner: Signer<'info>,

    #[account(
        mut,
        seeds = [b"vault_state", owner.key().as_ref(), vault_state.mint.as_ref()],
        bump = vault_state.bump,
        has_one = owner @ VaultError::Unauthorized,
    )]
    pub vault_state: Account<'info, VaultState>,

    #[account(
        mut,
        constraint = owner_token_account.mint == vault_state.mint @ VaultError::MintMismatch,
        constraint = owner_token_account.owner == owner.key()     @ VaultError::Unauthorized,
    )]
    pub owner_token_account: Account<'info, TokenAccount>,

    pub vault_token_account: Account<'info, TokenAccount>,

    pub token_program: Program<'info, Token>,
}

impl<'info> Withdraw<'info> {
    pub fn withdraw(&mut self, amount: u64) -> Result<()> {
        require!(amount > 0, VaultError::ZeroAmount);

        let clock = Clock::get()?;
        require!(
            clock.unix_timestamp >= self.vault_state.unlock_time,
            VaultError::VaultStillLocked,
        );

        require!(
            self.vault_token_account.amount >= amount,
            VaultError::InsufficientFunds,
        );

        let owner_key = self.owner.key();
        let mint_key = self.vault_state.mint;
        let seeds = &[
            b"vault_state".as_ref(),
            owner_key.as_ref(),
            mint_key.as_ref(),
            &[self.vault_state.bump],
        ];

        token::transfer(
            CpiContext::new_with_signer(
                self.token_program.key(), 
                Transfer {
                    from: self.vault_token_account.to_account_info(),
                    to: self.owner_token_account.to_account_info(),
                    authority: self.vault_state.to_account_info(),
                }, 
                &[&seeds[..]],
            ), 
            amount,
        )?;

        self.vault_state.total_deposited = self.vault_state.total_deposited
            .checked_add(amount)
            .ok_or(VaultError::Overflow)?;

        emit!(Withdrawn {
            owner: self.owner.key(),
            amount,
            remaining: self.vault_state.total_deposited,
        });
        
        Ok(())
    }
}