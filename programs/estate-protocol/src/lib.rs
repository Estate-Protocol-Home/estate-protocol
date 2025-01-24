use anchor_lang::prelude::*;
use anchor_spl::token::{Token, Mint, TokenAccount};
use mpl_token_metadata::instruction::{create_metadata_accounts_v3};
use anchor_spl::associated_token::AssociatedToken;
use anchor_spl::token::{self, Transfer};
use anchor_lang::solana_program::native_token::LAMPORTS_PER_SOL;
use pyth_sdk_solana::load_price_feed_from_account_info;


declare_id!("4WstPcHhmJed9upcqrZ9LpUSXBgx6qL4jP28pPdtCvie");

pub const ACCREDITED_LOCK_PERIOD: i64 = 365 * 24 * 60 * 60;    // 1 year in seconds
pub const NON_ACCREDITED_LOCK_PERIOD: i64 = 0; 
pub const PRECISION: u64 = 1_000_000;  // 6 decimals for USDC
pub const LAMPORTS_TO_SOL: u128 = LAMPORTS_PER_SOL as u128;

#[program]
pub mod estate_protocol {
   use super::*;

   pub fn initialize(ctx: Context<Initialize>) -> Result<()> {
       msg!("Greetings from: {:?}", ctx.program_id);
       Ok(())
   }

   pub fn create_security_token(
       ctx: Context<CreateSecurityToken>,
       name: String,
       symbol: String, 
       token_details: String,
       divisible: bool,
       treasury_wallet: Pubkey,
       document_hash: String,
   ) -> Result<()> {
       // Validate inputs
       require!(!name.is_empty() && name.len() <= 32, ErrorCode::InvalidName);
       require!(!symbol.is_empty() && symbol.len() <= 16, ErrorCode::InvalidSymbol);
       require!(!token_details.is_empty(), ErrorCode::InvalidTokenDetails);
       require!(!document_hash.is_empty(), ErrorCode::InvalidDocumentHash);
       require!(treasury_wallet != Pubkey::default(), ErrorCode::InvalidTreasuryWallet);

       // Create Metaplex metadata
       let metadata_program_key = ctx.accounts.token_metadata_program.key();
       let mint_key = ctx.accounts.mint.key();

       let seeds = &[
           b"metadata",
           metadata_program_key.as_ref(),
           mint_key.as_ref(),
       ];

       let (metadata_account, _) = Pubkey::find_program_address(seeds, &metadata_program_key);

       let metadata_ix = create_metadata_accounts_v3(
           metadata_program_key,
           metadata_account,
           mint_key,
           ctx.accounts.authority.key(),
           ctx.accounts.authority.key(),
           ctx.accounts.authority.key(),
           name.clone(),
           symbol.clone(),
           token_details.clone(), 
           None,                  // Creators not needed for this case
           0,                     // No seller fees
           true,                  // Update authority is signer
           true,                  // Metadata can be updated
           None,                  // No collection
           None,                  // No uses
           None                   // No collection details
       );

       anchor_lang::solana_program::program::invoke(
           &metadata_ix,
           &[
               ctx.accounts.metadata.to_account_info(),
               ctx.accounts.mint.to_account_info(),
               ctx.accounts.authority.to_account_info(),
               ctx.accounts.token_metadata_program.to_account_info(),
               ctx.accounts.token_program.to_account_info(),
               ctx.accounts.system_program.to_account_info(),
               ctx.accounts.rent.to_account_info(),
           ],
       )?;

       // Configure token
       let token_config = &mut ctx.accounts.token_config;
       token_config.authority = ctx.accounts.authority.key();
       token_config.mint = ctx.accounts.mint.key();
       token_config.name = name;
       token_config.symbol = symbol.clone();
       token_config.decimals = if divisible { 9 } else { 0 };
       token_config.token_details = token_details;
       token_config.document_hash = document_hash;
       token_config.treasury_wallet = treasury_wallet;
       token_config.status = TokenStatus::Created;
       token_config.bump = *ctx.bumps.get("token_config").unwrap();

       msg!("Security token created: {}", symbol);
       Ok(())
   }

   pub fn create_sto(
    ctx: Context<CreateSTO>,
    params: STOParameters,
) -> Result<()> {
    // Time validation
    let clock = Clock::get()?;
    let current_time = clock.unix_timestamp;
    require!(
        params.start_time > current_time,
        ErrorCode::InvalidStartTime
    );
    require!(
        params.end_time > params.start_time,
        ErrorCode::InvalidEndTime
    );

    // Tier validation
    require!(
        params.num_tiers > 0 && params.num_tiers <= STOConfig::MAX_TIERS as u8,
        ErrorCode::InvalidTierConfiguration
    );
    
    let mut total_allocation = 0u64;
    for i in 0..params.num_tiers as usize {
        let tier_params = &params.tiers[i];
        // Validate tier parameters
        require!(tier_params.rate > 0, ErrorCode::InvalidPrice);
        require!(
            tier_params.rate_discounted <= tier_params.rate,
            ErrorCode::InvalidDiscountRate
        );
        require!(
            tier_params.tokens_discounted <= tier_params.total_tokens,
            ErrorCode::InvalidDiscountAllocation
        );
        require!(
            tier_params.min_investment <= tier_params.max_investment,
            ErrorCode::InvalidInvestmentLimits
        );
        
        // Calculate total allocation
        total_allocation = total_allocation
            .checked_add(tier_params.total_tokens)
            .ok_or(ErrorCode::CalculationError)?;
    }
    
    require!(
        params.treasury_wallet != Pubkey::default(),
        ErrorCode::InvalidTreasuryWallet
    );

    // Initialize STO config
    let sto_config = &mut ctx.accounts.sto_config;
    sto_config.authority = ctx.accounts.authority.key();
    sto_config.token_mint = ctx.accounts.token_mint.key();
    sto_config.usdc_mint = ctx.accounts.usdc_mint.key();
    sto_config.treasury_wallet = params.treasury_wallet;
    sto_config.start_time = params.start_time;
    sto_config.end_time = params.end_time;
    sto_config.status = STOStatus::Created;
    sto_config.total_allocation = total_allocation;
    sto_config.total_sold = 0;
    sto_config.total_funds_raised = 0;
    sto_config.investor_count = 0;
    sto_config.whitelist_required = params.whitelist_required;
    sto_config.current_tier = 0;
    sto_config.max_tiers = params.num_tiers;

    // Initialize payment config
    sto_config.payment_mints = params.payment_mints;
    sto_config.payment_enabled = params.payment_enabled;
    sto_config.funds_raised = [0; 3];

    
    // Initialize tiers with default values first
    sto_config.tiers = [Tier::default(); STOConfig::MAX_TIERS];
    
    // Then fill in the actual tiers
    for i in 0..params.num_tiers as usize {
        sto_config.tiers[i] = Tier {
            rate: params.tiers[i].rate,
            rate_discounted: params.tiers[i].rate_discounted,
            total_tokens: params.tiers[i].total_tokens,
            tokens_sold: 0,
            tokens_discounted: params.tiers[i].tokens_discounted,
            min_investment: params.tiers[i].min_investment,
            max_investment: params.tiers[i].max_investment,
        };
    }
    
    sto_config.bump = *ctx.bumps.get("sto_config").unwrap();

    // Mint total allocation to Treasury PDA
    anchor_spl::token::mint_to(
        CpiContext::new_with_signer(
            ctx.accounts.token_program.to_account_info(),
            anchor_spl::token::MintTo {
                mint: ctx.accounts.token_mint.to_account_info(),
                to: ctx.accounts.sto_treasury.to_account_info(),
                authority: ctx.accounts.authority.to_account_info(),
            },
            &[&[
                b"sto_config",
                ctx.accounts.token_mint.key().as_ref(),
                &[sto_config.bump],
            ]],
        ),
        total_allocation,
    )?;

    msg!("STO created with {} tiers", params.num_tiers);
    Ok(())
}

pub fn activate_sto(ctx: Context<ManageSTO>) -> Result<()> {
    let sto_config = &mut ctx.accounts.sto_config;
    require!(sto_config.status == STOStatus::Created, ErrorCode::InvalidSTOStatus);
    
    let clock = Clock::get()?;
    require!(clock.unix_timestamp < sto_config.end_time, ErrorCode::STOExpired);
    
    sto_config.status = STOStatus::Active;
    emit!(STOStatusChanged { 
        sto: ctx.accounts.sto_config.key(),
        status: STOStatus::Active 
    });
    Ok(())
}

pub fn pause_sto(ctx: Context<ManageSTO>) -> Result<()> {
    let sto_config = &mut ctx.accounts.sto_config;
    require!(sto_config.status == STOStatus::Active, ErrorCode::InvalidSTOStatus);
    
    sto_config.status = STOStatus::Paused;
    emit!(STOStatusChanged { 
        sto: ctx.accounts.sto_config.key(),
        status: STOStatus::Paused 
    });
    Ok(())
}

pub fn complete_sto(ctx: Context<ManageSTO>) -> Result<()> {
    let sto_config = &mut ctx.accounts.sto_config;
    require!(
        sto_config.status == STOStatus::Active || 
        sto_config.status == STOStatus::Paused, 
        ErrorCode::InvalidSTOStatus
    );
    
    sto_config.status = STOStatus::Completed;
    emit!(STOStatusChanged { 
        sto: ctx.accounts.sto_config.key(),
        status: STOStatus::Completed 
    });
    Ok(())
}


pub fn invest(ctx: Context<Invest>, amount: u64, is_using_discount: bool) -> Result<()> {
    let tier_idx = ctx.accounts.sto_config.current_tier as usize;
    
    require!(ctx.accounts.sto_config.status == STOStatus::Active, ErrorCode::STONotActive);
    
    let clock = Clock::get()?;
    require!(
        clock.unix_timestamp >= ctx.accounts.sto_config.start_time &&
        clock.unix_timestamp <= ctx.accounts.sto_config.end_time,
        ErrorCode::OutsideSTOTime
    );
 
    let current_tier = &ctx.accounts.sto_config.tiers[tier_idx];
    
    require!(amount >= current_tier.min_investment, ErrorCode::BelowMinimumPurchase);
    require!(amount <= current_tier.max_investment, ErrorCode::ExceedsMaxInvestment);
    
    let tier_rate = if is_using_discount {
        require!(current_tier.tokens_discounted > 0, ErrorCode::InsufficientDiscountTokens);
        current_tier.rate_discounted
    } else {
        current_tier.rate  
    };
 
    let tokens_to_transfer = amount
        .checked_mul(PRECISION)
        .ok_or(ErrorCode::CalculationError)?
        .checked_div(tier_rate)
        .ok_or(ErrorCode::CalculationError)?;
 
    require!(
        tokens_to_transfer <= current_tier.total_tokens.saturating_sub(current_tier.tokens_sold),
        ErrorCode::ExceedsAllocation
    );
 
    token::transfer(
        CpiContext::new(
            ctx.accounts.token_program.to_account_info(),
            Transfer {
                from: ctx.accounts.investor_usdc_account.to_account_info(),
                to: ctx.accounts.treasury_usdc_account.to_account_info(),
                authority: ctx.accounts.investor.to_account_info(),
            },
        ),
        amount,
    )?;
 
    token::transfer(
        CpiContext::new_with_signer(
            ctx.accounts.token_program.to_account_info(),
            Transfer {
                from: ctx.accounts.sto_treasury.to_account_info(),
                to: ctx.accounts.investor_token_account.to_account_info(), 
                authority: ctx.accounts.sto_config.to_account_info(),
            },
            &[&[
                b"sto_config",
                ctx.accounts.token_mint.key().as_ref(),
                &[ctx.accounts.sto_config.bump],
            ]],
        ),
        tokens_to_transfer,
    )?;
 
    update_sto_state(
        &mut ctx.accounts.sto_config,
        tier_idx,
        tokens_to_transfer, 
        amount,
        is_using_discount
    )?;
 
    emit!(InvestmentMade {
        investor: ctx.accounts.investor.key(),
        amount,
        tokens_purchased: tokens_to_transfer
    });
 
    Ok(())
 }

 pub fn invest_with_sol(ctx: Context<InvestWithSol>, amount: u64) -> Result<()> {
    require!(
        ctx.accounts.sto_config.payment_enabled[0], 
        ErrorCode::PaymentNotEnabled
    );

    let tier_idx = ctx.accounts.sto_config.current_tier as usize;
    
    require!(ctx.accounts.sto_config.status == STOStatus::Active, ErrorCode::STONotActive);
    
    let clock = Clock::get()?;
    require!(
        clock.unix_timestamp >= ctx.accounts.sto_config.start_time &&
        clock.unix_timestamp <= ctx.accounts.sto_config.end_time,
        ErrorCode::OutsideSTOTime
    );

    // Get SOL/USD price from Pyth
    let price_feed = pyth_sdk_solana::load_price_feed_from_account_info(&ctx.accounts.sol_price_feed)
        .map_err(|_| ErrorCode::InvalidPriceFeed)?;
    let price = price_feed.get_price_unchecked();
    
    // Convert SOL amount to USD value using PRECISION
    let amount_usd = (amount as u128)
        .checked_mul(price.price as u128)
        .ok_or(ErrorCode::CalculationError)?
        .checked_div(LAMPORTS_TO_SOL as u128)
        .ok_or(ErrorCode::CalculationError)? as u64;

    let current_tier = &ctx.accounts.sto_config.tiers[tier_idx];
    
    require!(amount_usd >= current_tier.min_investment, ErrorCode::BelowMinimumPurchase);
    require!(amount_usd <= current_tier.max_investment, ErrorCode::ExceedsMaxInvestment);
    
    let tokens_to_transfer = amount_usd
        .checked_mul(PRECISION)
        .ok_or(ErrorCode::CalculationError)?
        .checked_div(current_tier.rate)
        .ok_or(ErrorCode::CalculationError)?;

    require!(
        tokens_to_transfer <= current_tier.total_tokens.saturating_sub(current_tier.tokens_sold),
        ErrorCode::ExceedsAllocation
    );

    // Transfer SOL to vault
    system_program::transfer(
        CpiContext::new(
            ctx.accounts.system_program.to_account_info(),
            system_program::Transfer {
                from: ctx.accounts.investor.to_account_info(),
                to: ctx.accounts.sol_vault.to_account_info(),
            },
        ),
        amount,
    )?;

    // Transfer security tokens to investor
    token::transfer(
        CpiContext::new_with_signer(
            ctx.accounts.token_program.to_account_info(),
            Transfer {
                from: ctx.accounts.sto_treasury.to_account_info(),
                to: ctx.accounts.investor_token_account.to_account_info(),
                authority: ctx.accounts.sto_config.to_account_info(),
            },
            &[&[
                b"sto_config",
                ctx.accounts.token_mint.key().as_ref(),
                &[ctx.accounts.sto_config.bump],
            ]],
        ),
        tokens_to_transfer,
    )?;

    // Update state
    let sto_config = &mut ctx.accounts.sto_config;
    sto_config.funds_raised[0] = sto_config.funds_raised[0].checked_add(amount).unwrap();
    
    update_sto_state(
        sto_config,
        tier_idx,
        tokens_to_transfer,
        amount_usd,
        false
    )?;

    emit!(InvestmentMade {
        investor: ctx.accounts.investor.key(),
        amount: amount_usd,
        tokens_purchased: tokens_to_transfer 
    });

    Ok(())
}

pub fn unfreeze_investor_tokens(ctx: Context<UnfreezeTokens>) -> Result<()> {
    let lock_status = &ctx.accounts.lock_status;
    let clock = Clock::get()?;
    
    if lock_status.is_accredited {
        // For accredited investors, check time-based lock
        require!(
            clock.unix_timestamp >= lock_status.unlock_time,
            ErrorCode::TokensStillLocked
        );
    } else {
        // For non-accredited, check if STO is completed
        require!(
            ctx.accounts.sto_config.status == STOStatus::Completed,
            ErrorCode::STONotCompleted
        );
    }

    // Unfreeze token account
    anchor_spl::token::thaw_account(
        CpiContext::new_with_signer(
            ctx.accounts.token_program.to_account_info(),
            anchor_spl::token::ThawAccount {
                account: ctx.accounts.investor_token_account.to_account_info(),
                mint: ctx.accounts.token_mint.to_account_info(),
                authority: ctx.accounts.token_config.to_account_info(),
            },
            &[&[
                b"token_config",
                ctx.accounts.token_mint.key().as_ref(),
                &[ctx.accounts.token_config.bump],
            ]],
        )
    )?;

    emit!(TokensUnfrozen {
        investor: ctx.accounts.investor.key(),
        is_accredited: lock_status.is_accredited,
    });

    Ok(())
}
   
}



fn update_sto_state(
    sto_config: &mut Account<STOConfig>,
    current_tier_idx: usize,
    tokens_purchased: u64,
    amount: u64,
    is_using_discount: bool,
) -> Result<()> {
    let tier = &mut sto_config.tiers[current_tier_idx];
    
    tier.tokens_sold = tier.tokens_sold.checked_add(tokens_purchased).unwrap();
    if is_using_discount {
        tier.tokens_discounted = tier.tokens_discounted.checked_sub(tokens_purchased).unwrap();
    }

    if tier.tokens_sold >= tier.total_tokens {
        sto_config.current_tier = sto_config.current_tier.checked_add(1).unwrap();
    }

    sto_config.total_sold = sto_config.total_sold.checked_add(tokens_purchased).unwrap();
    sto_config.total_funds_raised = sto_config.total_funds_raised.checked_add(amount).unwrap();
    sto_config.investor_count = sto_config.investor_count.checked_add(1).unwrap();
    
    Ok(())
}



#[derive(Accounts)]
pub struct Initialize<'info> {
   #[account(mut)]
   pub authority: Signer<'info>,
   pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
#[instruction(
   name: String,
   symbol: String,
   token_details: String, 
   divisible: bool,
   treasury_wallet: Pubkey,
   document_hash: String,
)]
pub struct CreateSecurityToken<'info> {
   #[account(mut)]
   pub authority: Signer<'info>,

   #[account(
       init,
       payer = authority,
       mint::decimals = if divisible { 9 } else { 0 },
       mint::authority = authority,
       mint::freeze_authority = authority,
   )]
   pub mint: Account<'info, Mint>,

   /// CHECK: Account checked in CPI
   #[account(mut)]
   pub metadata: UncheckedAccount<'info>,

   #[account(
       init,
       payer = authority,
       space = TokenConfig::LEN,
       seeds = [b"token_config", mint.key().as_ref()],
       bump,
   )]
   pub token_config: Account<'info, TokenConfig>,

   pub token_program: Program<'info, Token>,
   
   /// CHECK: Using official Metaplex program
   #[account(address = mpl_token_metadata::ID)]
   pub token_metadata_program: UncheckedAccount<'info>,

   pub system_program: Program<'info, System>,
   pub rent: Sysvar<'info, Rent>,
}


#[derive(Accounts)]
#[instruction(params: STOParameters)]
pub struct CreateSTO<'info> {
    #[account(
        mut,
        constraint = token_config.authority == authority.key() @ ErrorCode::UnauthorizedAuthority
    )]
    pub authority: Signer<'info>,

    #[account(
        init,
        payer = authority,
        space = STOConfig::LEN,
        seeds = [b"sto_config", token_mint.key().as_ref()],
        bump
    )]
    pub sto_config: Account<'info, STOConfig>,

    #[account(
        seeds = [b"token_config", token_mint.key().as_ref()],
        bump = token_config.bump,
        constraint = token_config.status == TokenStatus::Active @ ErrorCode::InvalidTokenStatus,
    )]
    pub token_config: Account<'info, TokenConfig>,

    pub token_mint: Account<'info, Mint>,
    pub usdc_mint: Account<'info, Mint>,

    #[account(
        init,
        payer = authority,
        associated_token::mint = token_mint,
        associated_token::authority = sto_config
    )]
    pub sto_treasury: Account<'info, TokenAccount>,
    
    pub token_program: Program<'info, Token>,
    pub associated_token_program: Program<'info, AssociatedToken>,
    pub system_program: Program<'info, System>,
    pub rent: Sysvar<'info, Rent>,
}

#[derive(Accounts)]
pub struct ManageSTO<'info> {
    #[account(
        mut,
        constraint = sto_config.authority == authority.key() @ ErrorCode::UnauthorizedAuthority
    )]
    pub authority: Signer<'info>,

    #[account(
        mut,
        seeds = [b"sto_config", token_mint.key().as_ref()],
        bump = sto_config.bump
    )]
    pub sto_config: Account<'info, STOConfig>,

    pub token_mint: Account<'info, Mint>,
}

#[derive(Accounts)]
#[instruction(amount: u64, is_using_discount: bool)]
pub struct Invest<'info> {
    #[account(mut)]
    pub investor: Signer<'info>,
    
    #[account(mut)]
    pub sto_config: Box<Account<'info, STOConfig>>, // Box the large account

    #[account(mut)]
    pub token_config: Account<'info, TokenConfig>,

    #[account(
        mut,
        constraint = investor_usdc_account.owner == investor.key()
    )]
    pub investor_usdc_account: Account<'info, TokenAccount>,

    #[account(mut)]
    pub treasury_usdc_account: Account<'info, TokenAccount>,

    #[account(
        mut,
        constraint = sto_treasury.mint == token_mint.key()
    )]
    pub sto_treasury: Account<'info, TokenAccount>,

    #[account(
        mut,
        constraint = investor_token_account.owner == investor.key(),
        constraint = investor_token_account.mint == token_mint.key()
    )]
    pub investor_token_account: Account<'info, TokenAccount>,

    pub token_mint: Account<'info, Mint>,
    pub usdc_mint: Account<'info, Mint>,
    pub token_program: Program<'info, Token>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct FreezeTokens<'info> {
    #[account(mut)] 
    pub investor: Signer<'info>,

    #[account(mut)]
    pub investor_token_account: Account<'info, TokenAccount>,

    #[account(
        init,
        payer = investor,
        space = InvestorLockStatus::LEN,
        seeds = [b"lock_status", investor.key().as_ref(), token_mint.key().as_ref()],
        bump
    )]
    pub lock_status: Account<'info, InvestorLockStatus>,

    #[account(
        seeds = [b"token_config", token_mint.key().as_ref()],
        bump = token_config.bump
    )]
    pub token_config: Account<'info, TokenConfig>,

    pub token_mint: Account<'info, Mint>,
    pub token_program: Program<'info, Token>,
    pub system_program: Program<'info, System>,
    pub rent: Sysvar<'info, Rent>,
}

#[derive(Accounts)]
pub struct UnfreezeTokens<'info> {
    pub investor: Signer<'info>,

    #[account(
        mut,
        constraint = investor_token_account.owner == investor.key(),
        constraint = investor_token_account.mint == token_mint.key()
    )]
    pub investor_token_account: Account<'info, TokenAccount>,

    #[account(
        seeds = [b"lock_status", investor.key().as_ref(), token_mint.key().as_ref()],
        bump = lock_status.bump,
        constraint = lock_status.investor == investor.key(),
        constraint = lock_status.token_mint == token_mint.key()
    )]
    pub lock_status: Account<'info, InvestorLockStatus>,

    #[account(
        seeds = [b"token_config", token_mint.key().as_ref()],
        bump = token_config.bump
    )]
    pub token_config: Account<'info, TokenConfig>,

    #[account(
        seeds = [b"sto_config", token_mint.key().as_ref()],
        bump = sto_config.bump
    )]
    pub sto_config: Account<'info, STOConfig>,

    pub token_mint: Account<'info, Mint>,
    pub token_program: Program<'info, Token>,
}

#[derive(Accounts)]
pub struct InvestWithSol<'info> {
   #[account(mut)]
   pub investor: Signer<'info>,
   
   #[account(mut)]
   pub sto_config: Box<Account<'info, STOConfig>>,

   #[account(mut)]
   pub token_config: Account<'info, TokenConfig>,

   #[account(
       mut,
       seeds = [b"sol_vault", sto_config.key().as_ref()],
       bump
   )]
   /// CHECK: SOL vault PDA
   pub sol_vault: AccountInfo<'info>,

   #[account(
       mut,
       constraint = sto_treasury.mint == token_mint.key()
   )]
   pub sto_treasury: Account<'info, TokenAccount>,

   #[account(
       mut,
       constraint = investor_token_account.owner == investor.key(),
       constraint = investor_token_account.mint == token_mint.key()
   )]
   pub investor_token_account: Account<'info, TokenAccount>,

   pub token_mint: Account<'info, Mint>,

   /// CHECK: Pyth price feed
   pub sol_price_feed: AccountInfo<'info>,
   
   pub token_program: Program<'info, Token>,
   pub system_program: Program<'info, System>,
}

#[account]
#[derive(Copy)] 
pub struct Tier {
    pub rate: u64,                // Price in USDC
    pub rate_discounted: u64,     // Discounted price (optional)
    pub total_tokens: u64,        // Total tokens in tier
    pub tokens_sold: u64,         // Tokens sold in tier
    pub tokens_discounted: u64,   // Discount allocation
    pub min_investment: u64,      // Min investment
    pub max_investment: u64,      // Max investment
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone, PartialEq)]
pub enum FundRaiseType {
    SOL,
    USDC,
    USDT
}

#[derive(AnchorSerialize, AnchorDeserialize)]
pub struct STOParameters {
    pub treasury_wallet: Pubkey,
    pub payment_mints: [Pubkey; 2], // [USDC, USDT]
    pub payment_enabled: [bool; 3], // [SOL, USDC, USDT]
    pub tiers: [TierParams; 1], 
    pub num_tiers: u8,
    pub start_time: i64,
    pub end_time: i64,
    pub whitelist_required: bool,
}
#[derive(AnchorSerialize, AnchorDeserialize)]
pub struct TierParams {
    pub rate: u64,
    pub rate_discounted: u64,
    pub total_tokens: u64,
    pub tokens_discounted: u64,
    pub min_investment: u64,
    pub max_investment: u64,
}

#[account]
#[derive(Default)]
pub struct TokenConfig {
   pub authority: Pubkey,          
   pub mint: Pubkey,              
   pub name: String,              
   pub symbol: String,            
   pub decimals: u8,              
   pub token_details: String,     
   pub document_hash: String,     
   pub treasury_wallet: Pubkey,   
   pub status: TokenStatus,       
   pub bump: u8,                  
}

#[account]
#[derive(Default)]
pub struct STOConfig {
    pub authority: Pubkey,        
    pub token_mint: Pubkey,       
    pub usdc_mint: Pubkey,        
    pub treasury_wallet: Pubkey,  
    pub total_allocation: u64,
    pub total_sold: u64,         
    pub total_funds_raised: u64,      
    pub investor_count: u32,          
    pub start_time: i64,         
    pub end_time: i64,           
    pub status: STOStatus,       
    pub whitelist_required: bool,   
    pub payment_mints: [Pubkey; 2],  // [USDC, USDT]
    pub payment_enabled: [bool; 3],   // [SOL, USDC, USDT] 
    pub funds_raised: [u64; 3],      // Amount raised per payment type  
    pub current_tier: u8,         
    pub max_tiers: u8,           
    pub tiers: [Tier; 1],
    pub bump: u8,                
}



#[account]
#[derive(Default)]
pub struct InvestorLockStatus {
    pub investor: Pubkey,
    pub token_mint: Pubkey,
    pub unlock_time: i64,
    pub is_accredited: bool,
    pub bump: u8,
}
#[account]
#[derive(Default)]
pub struct InvestorDiscount {
    pub investor: Pubkey,
    pub is_eligible: bool,
    pub discount_amount: u64,
    pub bump: u8,
}


#[derive(AnchorSerialize, AnchorDeserialize, Clone, PartialEq, Default)]
pub enum TokenStatus {
   #[default]
   Created,
   Active, 
   Paused,
   Frozen,
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone, PartialEq, Default)]
pub enum STOStatus {
    #[default]
    Created,    // Initial state after creation
    Active,     // STO is live and accepting investments
    Paused,     // Temporarily suspended
    Completed,  // Successfully ended
    Cancelled   // Permanently stopped
}

#[error_code]
pub enum ErrorCode {
   #[msg("Name cannot be empty or longer than 32 characters")]
   InvalidName,
   #[msg("Symbol cannot be empty or longer than 16 characters")] 
   InvalidSymbol,
   #[msg("Token details cannot be empty")]
   InvalidTokenDetails,
   #[msg("Document hash cannot be empty")]
   InvalidDocumentHash,
   #[msg("Invalid treasury wallet address")]
   InvalidTreasuryWallet,
   #[msg("Invalid start time - must be in the future")]
   InvalidStartTime,
   #[msg("Invalid end time - must be after start time")]
   InvalidEndTime,
   #[msg("Invalid purchase amount")]
   InvalidPurchaseAmount,
   #[msg("Invalid price")]
   InvalidPrice,
   #[msg("Invalid allocation")]
   InvalidAllocation,
   #[msg("Invalid non-accredited investor limit")]
   InvalidNonAccreditedLimit,
   #[msg("Unauthorized authority")]
   UnauthorizedAuthority,
   #[msg("Invalid token status")]
   InvalidTokenStatus,
   #[msg("Invalid STO status for this operation")]
   InvalidSTOStatus,
   #[msg("STO has expired")]
   STOExpired,
   #[msg("STO is not active")]
   STONotActive,
   #[msg("Outside STO time window")]
   OutsideSTOTime,
   #[msg("Amount below minimum purchase")]
   BelowMinimumPurchase,
   #[msg("Calculation error")]
   CalculationError,
   #[msg("Exceeds total allocation")]
    ExceedsAllocation,
    #[msg("Tokens are still locked")]
    TokensStillLocked,
    #[msg("STO must be completed to unfreeze non-accredited tokens")]
    STONotCompleted,
    #[msg("Invalid tier configuration")]
    InvalidTierConfiguration,
    #[msg("Invalid discount rate")]
    InvalidDiscountRate,
    #[msg("Invalid discount allocation")]
    InvalidDiscountAllocation,
    #[msg("Tier is full")]
    TierFull,
    #[msg("Invalid tier")]
    InvalidTier,
    #[msg("Not eligible for discount")]
    NotEligibleForDiscount,
    #[msg("Insufficient discount tokens")]
    InsufficientDiscountTokens,
    #[msg("Exceeds maximum investment")]
    ExceedsMaxInvestment,
    #[msg("Invalid investment limits configuration")]
    InvalidInvestmentLimits,
    #[msg("Payment type not enabled")]
   PaymentNotEnabled,
    #[msg("Invalid price feed")]
   InvalidPriceFeed,
}

#[event]
pub struct STOStatusChanged {
    pub sto: Pubkey,
    pub status: STOStatus,
}

#[event]
pub struct InvestmentMade {
    pub investor: Pubkey,
    pub amount: u64,
    pub tokens_purchased: u64,
}

#[event]
pub struct TokensFrozen {
    pub investor: Pubkey,
    pub unlock_time: i64,
    pub is_accredited: bool,
}

#[event]
pub struct TokensUnfrozen {
    pub investor: Pubkey,
    pub is_accredited: bool,
}

impl TokenConfig {
   pub const LEN: usize = 8 +      // discriminator
       32 +                        // authority
       32 +                        // mint
       32 +                        // name string
       16 +                        // symbol string
       1 +                         // decimals  
       128 +                       // token_details string
       64 +                        // document_hash string
       32 +                        // treasury_wallet
       1 +                         // status enum
       1;                          // bump
}


impl STOConfig {
    pub const MAX_TIERS: usize = 1;
    
    pub const LEN: usize = 8 +    // Discriminator
        32 +                      // authority: Pubkey
        32 +                      // token_mint: Pubkey  
        32 +                      // usdc_mint: Pubkey
        32 +                      // treasury_wallet: Pubkey
        (32 * 2) +               // payment_mints array
        3 +                      // payment_enabled array 
        (8 * 3) +                // funds_raised array
        8 +                       // total_allocation: u64
        8 +                       // total_sold: u64
        8 +                       // total_funds_raised: u64
        4 +                       // investor_count: u32
        8 +                       // start_time: i64
        8 +                       // end_time: i64
        1 +                       // status: STOStatus
        1 +                       // whitelist_required: bool
        1 +                       // current_tier: u8
        1 +                       // max_tiers: u8
        (8 * 7) +                // tiers: [Tier; MAX_TIERS] (7 u64 fields * 8 bytes)
        1;                       // bump: u8
}

impl InvestorLockStatus {
    pub const LEN: usize = 8 +    // Discriminator
        32 +                      // investor
        32 +                      // token_mint
        8 +                       // unlock_time
        1 +                       // is_accredited
        1;                        // bump
}

impl Tier {
    pub const LEN: usize = 8 +
        8 +                       // rate
        8 +                       // rate_discounted
        8 +                       // total_tokens
        8 +                       // tokens_sold
        8 +                       // tokens_discounted
        8 +                       // min_investment
        8;                        // max_investment
}

impl Default for Tier {
    fn default() -> Self {
        Self {
            rate: 0,
            rate_discounted: 0,
            total_tokens: 0,
            tokens_sold: 0,
            tokens_discounted: 0,
            min_investment: 0,
            max_investment: 0,
        }
    }
}
