use anchor_lang::prelude::*;
use anchor_spl::token::{self, CloseAccount, Token, TokenAccount, Transfer};

use crate::{VaultError, VaultState, events::VaultClosed};

#[derive(Accounts)]
pub struct Close<'info> {
    #[account(mut)]
    pub owner: Signer<'info>,

    #[account(
        mut,
        seeds = [b"vault_state", owner.key().as_ref(), vault_state.mint.as_ref()],
        bump = vault_state.bump,
        has_one = owner @VaultError::Unauthorized,
        close = owner,
    )]
    pub vault_state: Account<'info, VaultState>,

    #[account(
        mut,
        constraint = owner_token_account.mint == vault_state.mint @ VaultError::MintMismatch,
        constraint = owner_token_account.owner == owner.key()     @ VaultError::Unauthorized,
    )]
    pub owner_token_account: Account<'info, TokenAccount>,

    #[account(
        mut,
        seeds = [b"vault", owner.key().as_ref(), vault_state.mint.as_ref()],
        bump = vault_state.vault_bump,
    )]
    pub vault_token_account: Account<'info, TokenAccount>,

    pub token_program: Program<'info, Token>,
    pub system_program: Program<'info, System>,
}

impl<'info> Close<'info> {
    pub fn close(&mut self) -> Result<()> {
        let clock = Clock::get()?;
        require!(
            clock.unix_timestamp >= self.vault_state.unlock_time,
            VaultError::VaultStillLocked
        );

        let remaining = self.vault_token_account.amount;
        let owner_key = self.owner.key();
        let mint_key = self.vault_state.mint;

        let seeds = &[
            b"vault_state",
            owner_key.as_ref(),
            mint_key.as_ref(),
            &[self.vault_state.bump],
        ];

        if remaining > 0 {
            token::transfer(
                CpiContext::new_with_signer(
                    self.token_program.key(), 
                    Transfer {
                        from: self.vault_token_account.to_account_info(),
                        to: self.owner_token_account.to_account_info(),
                        authority: self.vault_state.to_account_info(),
                    }, 
                    &[&seeds[..]]
                ), 
                remaining
            )?;
        }

        token::close_account(
            CpiContext::new_with_signer(
                self.token_program.key(), 
                CloseAccount {
                    account: self.vault_token_account.to_account_info(),
                    destination: self.owner.to_account_info(),
                    authority: self.vault_state.to_account_info(),
                }, 
                &[&seeds[..]],
            ),
        )?;

        emit!(VaultClosed {
            owner: self.owner.key(),
            token_returned: remaining,
        });
    
        Ok(())
    }
}