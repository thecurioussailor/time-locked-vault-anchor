pub mod error;
pub mod instructions;
pub mod state;
pub mod events;

use anchor_lang::prelude::*;

pub use instructions::*;
pub use state::*;

declare_id!("8wVnNuJttbSMP6zEMsCosBFb5UsdG4BgEjFa1iD9D2LM");

#[program]
pub mod time_locked_vault {
    use super::*;

    pub fn initialize(ctx: Context<Initialize>) -> Result<()> {
        initialize::handler(ctx)
    }
}
