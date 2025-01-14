use anchor_lang::prelude::*;
use anchor_spl::token::{Token, Mint};
use mpl_token_metadata::instruction::{create_metadata_accounts_v3};

declare_id!("4WstPcHhmJed9upcqrZ9LpUSXBgx6qL4jP28pPdtCvie");

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

#[derive(AnchorSerialize, AnchorDeserialize, Clone, PartialEq, Default)]
pub enum TokenStatus {
   #[default]
   Created,
   Active, 
   Paused,
   Frozen,
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