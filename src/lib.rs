/// Read Description before you proceed so more and more people can Understand Gajesh's Token Vesting Program
use anchor_lang::prelude::*;
use anchor_lang::solana_program::{clock, program_option::COption, sysvar};
use anchor_spl::token::{self, Mint, Token, TokenAccount};

declare_id!("EYK2eucQ7A3npLEwWHPEqA9GhoieRERPRN6bRPVgocz2");

pub fn available(
    ticket: &mut Box<Account<Ticket>>,
) -> u64 {
    if has_cliffed(ticket) {
        return unlocked(ticket).checked_sub(ticket.claimed).unwrap();
    } else {
        return 0;
    }
}

pub fn has_cliffed(
    ticket: &mut Box<Account<Ticket>>,
) -> bool {
    let clock = clock::Clock::get().unwrap();
    if ticket.cliff == 0 {
        return true;
    }

    return clock.unix_timestamp as u64 > ticket.created_at.checked_add(
        ticket.cliff.checked_mul(
            86400
        ).unwrap()
    ).unwrap();
}

pub fn unlocked(
    ticket: &mut Box<Account<Ticket>>,
) -> u64 {
    let clock = clock::Clock::get().unwrap();
    
    let timelapsed = (clock.unix_timestamp as u64).checked_sub(ticket.created_at).unwrap();  
    let vesting_in_seconds = ticket.vesting.checked_mul(86400).unwrap();

    return timelapsed.checked_mul(ticket.amount).unwrap().checked_div(
        vesting_in_seconds as u64
    ).unwrap();
}

#[program]
pub mod vestor {
    use super::*;
    pub fn initialize(ctx: Context<Initialize>, nonce: u8) -> Result<()> {
        let vestor = &mut ctx.accounts.vestor;
        vestor.current_id = 1;
        vestor.nonce = nonce;

        Ok(())
    }

    pub fn create(ctx: Context<Create>, beneficiary: Pubkey, cliff: u64, vesting: u64, amount: u64, irrevocable: bool) -> Result<()> {
        let vestor = &mut ctx.accounts.vestor;
        let clock = clock::Clock::get().unwrap();

        if amount == 0 {
            return Err(ErrorCode::AmountMustBeGreaterThanZero.into());
        } if vesting < cliff {
            return Err(ErrorCode::VestingPeriodShouldBeEqualOrLongerThanCliff.into());
        } 

         // Transfer tokens to vault.
         {
            let cpi_ctx = CpiContext::new(
                ctx.accounts.token_program.to_account_info(),
                token::Transfer {
                    from: ctx.accounts.grantor_token_vault.to_account_info(),
                    to: ctx.accounts.token_vault.to_account_info(),
                    authority: ctx.accounts.grantor.to_account_info(), //todo use user account as signer
                },
            );
            token::transfer(cpi_ctx, amount)?;
        }

        vestor.current_id += 1;
        let ticket = &mut ctx.accounts.ticket;
        ticket.token_mint = ctx.accounts.token_mint.key();
        ticket.token_vault = ctx.accounts.token_vault.key();
        ticket.grantor = ctx.accounts.grantor.key();
        ticket.beneficiary = beneficiary;
        ticket.cliff = cliff;
        ticket.vesting = vesting;
        ticket.amount = amount;
        ticket.balance = amount;
        ticket.created_at = clock.unix_timestamp as u64;
        ticket.irrevocable = irrevocable;
        ticket.is_revoked = false;

        Ok(())
    }

    pub fn claim(ctx: Context<Claim>) -> Result<()> {
        let vestor = &mut ctx.accounts.vestor;
        let ticket = &mut ctx.accounts.ticket;
        let clock = clock::Clock::get().unwrap();

        if ticket.is_revoked == true {
            return Err(ErrorCode::TicketRevoked.into());
        }
        let amount = available(ticket);

        // Transfer.
        {
            let seeds = &[vestor.to_account_info().key.as_ref(), &[vestor.nonce]];
            let signer = &[&seeds[..]];

            let cpi_ctx = CpiContext::new_with_signer(
                ctx.accounts.token_program.to_account_info(),
                token::Transfer {
                    from: ctx.accounts.token_vault.to_account_info(),
                    to: ctx.accounts.beneficiary_token_vault.to_account_info(),
                    authority: ctx.accounts.signer.to_account_info(), 
                },
                signer
            );
            token::transfer(cpi_ctx, amount)?;
        }

        ticket.claimed += amount;
        ticket.balance -= amount;
        ticket.last_claimed_at = clock.unix_timestamp as u64;
        ticket.num_claims += 1;

        Ok(())
    }

    pub fn revoke(ctx: Context<Revoke>) -> Result<()> {
        let vestor = &mut ctx.accounts.vestor;
        let ticket = &mut ctx.accounts.ticket;
        let clock = clock::Clock::get().unwrap();

        if ticket.is_revoked == true {
            return Err(ErrorCode::TicketRevoked.into());
        } if ticket.irrevocable == true {
            return Err(ErrorCode::TicketIrrevocable.into());
        }

        // Transfer.
        {
            let seeds = &[vestor.to_account_info().key.as_ref(), &[vestor.nonce]];
            let signer = &[&seeds[..]];

            let cpi_ctx = CpiContext::new_with_signer(
                ctx.accounts.token_program.to_account_info(),
                token::Transfer {
                    from: ctx.accounts.token_vault.to_account_info(),
                    to: ctx.accounts.grantor_token_vault.to_account_info(),
                    authority: ctx.accounts.signer.to_account_info(), 
                },
                signer
            );
            token::transfer(cpi_ctx, ticket.balance)?;
        }

        ticket.is_revoked = true;
        ticket.balance = 0;

        Ok(())
    }
}

#[derive(Accounts)]
pub struct Initialize<'info> {
    #[account(init, payer = user, space = 8 + 8)]
    pub vestor: Account<'info, Vestor>,
    #[account(mut)]
    pub user: Signer<'info>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct Create<'info> {
    #[account(mut)]
    pub vestor: Account<'info, Vestor>,

    #[account(
        init_if_needed,
        payer = grantor,
        seeds = [
            vestor.to_account_info().key().as_ref(),
            vestor.current_id.to_string().as_ref(),
        ],
        bump
    )]
    pub ticket: Box<Account<'info, Ticket>>,

    pub token_mint: Box<Account<'info, Mint>>,
    #[account(
        constraint = token_vault.mint == token_mint.key(),
        constraint = token_vault.owner == signer.key(),
    )]
    pub token_vault: Box<Account<'info, TokenAccount>>,

    #[account(
        constraint = token_vault.mint == token_mint.key(),
        constraint = token_vault.owner == grantor.key(),
    )]
    pub grantor_token_vault: Box<Account<'info, TokenAccount>>,

    #[account(
        seeds = [
            vestor.to_account_info().key.as_ref()
        ],
        bump = vestor.nonce,
    )]
    pub signer: UncheckedAccount<'info>,

    #[account(mut)]
    pub grantor: Signer<'info>,

    pub system_program: Program<'info, System>,
    pub token_program: Program<'info, Token>,
}

#[derive(Accounts)]
pub struct Claim<'info> {
    #[account(mut)]
    pub vestor: Account<'info, Vestor>,

    #[account(
        mut,
        has_one = beneficiary,
        has_one = token_mint,
        has_one = token_vault,
        constraint = ticket.balance > 0,
        constraint = ticket.amount > 0,
    )]
    pub ticket: Box<Account<'info, Ticket>>,

    pub token_mint: Box<Account<'info, Mint>>,
    #[account(
        constraint = token_vault.mint == token_mint.key(),
        constraint = token_vault.owner == signer.key(),
    )]
    pub token_vault: Box<Account<'info, TokenAccount>>,

    #[account(
        constraint = token_vault.mint == token_mint.key(),
        constraint = token_vault.owner == beneficiary.key(),
    )]
    pub beneficiary_token_vault: Box<Account<'info, TokenAccount>>,

    #[account(
        seeds = [
            vestor.to_account_info().key.as_ref()
        ],
        bump = vestor.nonce,
    )]
    pub signer: UncheckedAccount<'info>,

    #[account(mut)]
    pub beneficiary: Signer<'info>,

    pub system_program: Program<'info, System>,
    pub token_program: Program<'info, Token>,
}

#[derive(Accounts)]
pub struct Revoke<'info> {
    #[account(mut)]
    pub vestor: Account<'info, Vestor>,

    #[account(
        mut,
        has_one = grantor,
        has_one = token_mint,
        has_one = token_vault,
        constraint = ticket.balance > 0,
    )]
    pub ticket: Box<Account<'info, Ticket>>,

    pub token_mint: Box<Account<'info, Mint>>,
    #[account(
        constraint = token_vault.mint == token_mint.key(),
        constraint = token_vault.owner == signer.key(),
    )]
    pub token_vault: Box<Account<'info, TokenAccount>>,

    #[account(
        constraint = token_vault.mint == token_mint.key(),
        constraint = token_vault.owner == grantor.key(),
    )]
    pub grantor_token_vault: Box<Account<'info, TokenAccount>>,

    #[account(
        seeds = [
            vestor.to_account_info().key.as_ref()
        ],
        bump = vestor.nonce,
    )]
    pub signer: UncheckedAccount<'info>,

    #[account(mut)]
    pub grantor: Signer<'info>,

    pub system_program: Program<'info, System>,
    pub token_program: Program<'info, Token>,
}

#[account]
pub struct Vestor {
    pub current_id: u64,

    pub nonce: u8
}

#[account]
#[derive(Default)]
pub struct Ticket {
    pub token_mint: Pubkey,
    pub token_vault: Pubkey,
    pub grantor: Pubkey,
    pub beneficiary: Pubkey,
    pub cliff: u64, 
    pub vesting: u64,
    pub amount: u64,
    pub claimed: u64,
    pub balance: u64,
    pub created_at: u64,
    pub last_claimed_at: u64,
    pub num_claims: u64,
    pub irrevocable: bool,
    pub is_revoked: bool,
    pub revoked_at: u64,
}

#[error_code]
pub enum ErrorCode {
    #[msg("Amount must be greater than zero.")]
    AmountMustBeGreaterThanZero,
    #[msg("Vesting period should be equal or longer to the cliff")]
    VestingPeriodShouldBeEqualOrLongerThanCliff,
    #[msg("Ticket has been revoked")]
    TicketRevoked,
    #[msg("Ticket is irrevocable")]
    TicketIrrevocable,
}
