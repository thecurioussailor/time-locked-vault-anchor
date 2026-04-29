use anchor_lang::prelude::*;

#[error_code]
pub enum VaultError {
    #[msg("You are not the owner of this vault")]
    Unauthorized,

    #[msg("Amount must be greater than zero")]
    ZeroAmount,

    #[msg("Vault is still locked - unlock time has not passed")]
    VaultStillLocked,

    #[msg("Insufficient tokens in vault")]
    InsufficientFunds,

    #[msg("Token mint does not match this vault")]
    MintMismatch,

    #[msg("Lock duration is too short (minimum 60 seconds)")]
    LockDurationTooShort,

    #[msg("Lock duration is too long (maximum 4 years)")]
    LockDurationTooLong,

    #[msg("Arithmetic Overflow")]
    Overflow,
}