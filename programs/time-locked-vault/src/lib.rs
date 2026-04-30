pub mod error;
pub mod instructions;
pub mod state;
pub mod events;

use anchor_lang::prelude::*;

pub use instructions::*;
pub use state::*;
pub use error::*;

declare_id!("8wVnNuJttbSMP6zEMsCosBFb5UsdG4BgEjFa1iD9D2LM");

#[program]
pub mod time_locked_vault {
    use super::*;

    pub fn initialize(ctx: Context<Initialize>, lock_duration: i64) -> Result<()> {
        ctx.accounts.initialize(&ctx.bumps, lock_duration)
    }

    pub fn deposit(ctx: Context<Deposit>, amount: u64) -> Result<()> {
        ctx.accounts.deposit(amount)
    }

    pub fn withdraw(ctx: Context<Withdraw>, amount: u64) -> Result<()> {
        ctx.accounts.withdraw(amount)
    }

    pub fn close(ctx: Context<Close>) -> Result<()> {
        ctx.accounts.close()
    }

    pub fn time_remaining(ctx: Context<TimeRemaining>) -> Result<i64> {
        ctx.accounts.time_remaining()
    }
}
