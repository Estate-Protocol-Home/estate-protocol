use anchor_lang::prelude::*;

declare_id!("FnYCJduVWVoSzg55RNG1y9up4ow7uHvJXmeGEjyZeyFb");

#[program]
pub mod estate_protocol {
    use super::*;

    pub fn initialize(ctx: Context<Initialize>) -> Result<()> {
        msg!("Greetings from: {:?}", ctx.program_id);
        Ok(())
    }
}

#[derive(Accounts)]
pub struct Initialize {}
