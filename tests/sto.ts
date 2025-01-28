import * as anchor from "@coral-xyz/anchor";
import { Program } from "@coral-xyz/anchor";
import { EstateProtocol, IDL } from "../target/types/estate_protocol";
import { PublicKey, SystemProgram, SYSVAR_RENT_PUBKEY, Keypair } from "@solana/web3.js";
import { TOKEN_PROGRAM_ID, ASSOCIATED_TOKEN_PROGRAM_ID, getAssociatedTokenAddress, createMint, createAccount, mintTo } from "@solana/spl-token";
import { expect } from "chai";

describe("Security Token Offering", () => {
  const provider = anchor.AnchorProvider.env();
  anchor.setProvider(provider);

  const program = new Program(
    IDL,
    new PublicKey("4WstPcHhmJed9upcqrZ9LpUSXBgx6qL4jP28pPdtCvie"),
    provider
  );

  //existing token mint
  const tokenMint = new PublicKey("DRquos8vPqKRdfTsF1NvYhnHV77KauHnGPHZJeQdYwju");
  
  const usdcMintKeypair = Keypair.generate();
  let usdcMint: PublicKey;
  let stoConfig: PublicKey;
  let stoTreasury: PublicKey;
  let tokenConfig: PublicKey;
  let investorUsdc: PublicKey;

  before(async () => {
    console.log("Setting up test environment...");

    // Derive token config PDA
    const [tokenConfigPDA] = await PublicKey.findProgramAddress(
      [Buffer.from("token_config"), tokenMint.toBuffer()],
      program.programId
    );
    tokenConfig = tokenConfigPDA;

    // Check token status and activate if needed
    try {
      const tokenConfigAccount = await program.account.tokenConfig.fetch(tokenConfig);
      console.log("Token status:", tokenConfigAccount.status);
      
      // If token exists but isn't active, activate it
      if (tokenConfigAccount.status.created) {
        console.log("Activating token...");
        await program.methods
          .activateToken()
          .accounts({
            authority: provider.wallet.publicKey,
            tokenConfig: tokenConfig,
            tokenMint: tokenMint,
          })
          .rpc();
        console.log("Token activated");
      }
    } catch (e) {
      console.error("Error checking token status:", e);
      throw new Error("Please ensure token is created first using createSecurityToken");
    }
    
    // Initialize mock USDC mint
    usdcMint = await createMint(
      provider.connection,
      (provider.wallet as any).payer,
      provider.wallet.publicKey,
      null,
      6,  // USDC has 6 decimals
      usdcMintKeypair
    );

    // Create USDC account for investor (testing account)
    investorUsdc = await createAccount(
      provider.connection,
      (provider.wallet as any).payer,
      usdcMint,
      provider.wallet.publicKey
    );

    // Mint some USDC to investor account for testing
    await mintTo(
      provider.connection,
      (provider.wallet as any).payer,
      usdcMint,
      investorUsdc,
      provider.wallet.publicKey,
      1000000000 // 1000 USDC
    );

    // Derive STO config PDA
    const [stoConfigPDA] = await PublicKey.findProgramAddress(
      [Buffer.from("sto_config"), tokenMint.toBuffer()],
      program.programId
    );
    stoConfig = stoConfigPDA;

    // Get associated token account for STO treasury
    stoTreasury = await getAssociatedTokenAddress(
      tokenMint,
      stoConfig,
      true
    );
  });

  it("Creates a security token offering", async () => {
    const now = Math.floor(Date.now() / 1000);

    const tierParams = {
      rate: new anchor.BN(1_000_000),
      rateDiscounted: new anchor.BN(900_000),
      totalTokens: new anchor.BN(1_000_000_000_000),
      tokensDiscounted: new anchor.BN(100_000_000_000),
      minInvestment: new anchor.BN(100_000_000),
      maxInvestment: new anchor.BN(100_000_000_000),
    };
    
    const stoParams = {
      treasuryWallet: provider.wallet.publicKey,
      paymentMints: [usdcMint, usdcMint] as [PublicKey, PublicKey],
      paymentEnabled: [true, true, false] as [boolean, boolean, boolean],
      tiers: [tierParams],
      numTiers: 1,
      startTime: new anchor.BN(now + 3600),
      endTime: new anchor.BN(now + (7 * 24 * 3600)),
      whitelistRequired: false,
    };

    try {
      await program.methods
        .createSto(stoParams)
        .accounts({
          authority: provider.wallet.publicKey,
          stoConfig,
          tokenConfig,
          tokenMint,
          usdcMint,
          stoTreasury,
          tokenProgram: TOKEN_PROGRAM_ID,
          associatedTokenProgram: ASSOCIATED_TOKEN_PROGRAM_ID,
          systemProgram: SystemProgram.programId,
          rent: SYSVAR_RENT_PUBKEY,
        })
        .rpc();

      // Verify STO config
      const stoConfigAccount = await program.account.stoConfig.fetch(stoConfig);
      
      expect(stoConfigAccount.authority).to.eql(provider.wallet.publicKey);
      expect(stoConfigAccount.tokenMint).to.eql(tokenMint);
      expect(stoConfigAccount.treasuryWallet).to.eql(provider.wallet.publicKey);
      expect(stoConfigAccount.startTime.toNumber()).to.eq(stoParams.startTime.toNumber());
      expect(stoConfigAccount.endTime.toNumber()).to.eq(stoParams.endTime.toNumber());
      expect(stoConfigAccount.whitelistRequired).to.eq(false);
      expect(stoConfigAccount.currentTier).to.eq(0);
      expect(stoConfigAccount.maxTiers).to.eq(1);

      // Verify tier configuration
      const tier = stoConfigAccount.tiers[0] as any;
      expect(tier.rate.toNumber()).to.eq(tierParams.rate.toNumber());
      expect(tier.rateDiscounted.toNumber()).to.eq(tierParams.rateDiscounted.toNumber());
      expect(tier.totalTokens.toNumber()).to.eq(tierParams.totalTokens.toNumber());
      expect(tier.tokensDiscounted.toNumber()).to.eq(tierParams.tokensDiscounted.toNumber());
      expect(tier.minInvestment.toNumber()).to.eq(tierParams.minInvestment.toNumber());
      expect(tier.maxInvestment.toNumber()).to.eq(tierParams.maxInvestment.toNumber());
    } catch (err) {
      console.error("Error creating STO:", err);
      throw err;
    }
  });

  it("Should fail with invalid start time", async () => {
    const now = Math.floor(Date.now() / 1000);

    const tierParams = {
      rate: new anchor.BN(1_000_000),
      rateDiscounted: new anchor.BN(900_000),
      totalTokens: new anchor.BN(1_000_000_000_000),
      tokensDiscounted: new anchor.BN(100_000_000_000),
      minInvestment: new anchor.BN(100_000_000),
      maxInvestment: new anchor.BN(100_000_000_000),
    };
    
    const invalidParams = {
      treasuryWallet: provider.wallet.publicKey,
      paymentMints: [usdcMint, usdcMint] as [PublicKey, PublicKey],
      paymentEnabled: [true, true, false] as [boolean, boolean, boolean],
      tiers: [tierParams],
      numTiers: 1,
      startTime: new anchor.BN(now - 3600), // Invalid: start time in the past
      endTime: new anchor.BN(now + (7 * 24 * 3600)),
      whitelistRequired: false,
    };

    try {
      await program.methods
        .createSto(invalidParams)
        .accounts({
          authority: provider.wallet.publicKey,
          stoConfig,
          tokenConfig,
          tokenMint,
          usdcMint,
          stoTreasury,
          tokenProgram: TOKEN_PROGRAM_ID,
          associatedTokenProgram: ASSOCIATED_TOKEN_PROGRAM_ID,
          systemProgram: SystemProgram.programId,
          rent: SYSVAR_RENT_PUBKEY,
        })
        .rpc();
      
      expect.fail("Should have failed with invalid start time");
    } catch (err) {
      expect(err.toString()).to.include("InvalidStartTime");
    }
  });

  it("Should activate STO successfully", async () => {
    try {
      await program.methods
        .activateSto()
        .accounts({
          authority: provider.wallet.publicKey,
          stoConfig,
          tokenMint,
        })
        .rpc();

      const stoConfigAccount = await program.account.stoConfig.fetch(stoConfig);
      expect(stoConfigAccount.status.active).to.be.true;
    } catch (err) {
      console.error("Error activating STO:", err);
      throw err;
    }
  });

  it("Should pause STO successfully", async () => {
    try {
      await program.methods
        .pauseSto()
        .accounts({
          authority: provider.wallet.publicKey,
          stoConfig,
          tokenMint,
        })
        .rpc();

      const stoConfigAccount = await program.account.stoConfig.fetch(stoConfig);
      expect(stoConfigAccount.status.paused).to.be.true;
    } catch (err) {
      console.error("Error pausing STO:", err);
      throw err;
    }
  });

  it("Should complete STO successfully", async () => {
    try {
      await program.methods
        .completeSto()
        .accounts({
          authority: provider.wallet.publicKey,
          stoConfig,
          tokenMint,
        })
        .rpc();

      const stoConfigAccount = await program.account.stoConfig.fetch(stoConfig);
      expect(stoConfigAccount.status.completed).to.be.true;
    } catch (err) {
      console.error("Error completing STO:", err);
      throw err;
    }
  });

  it("Should make an investment successfully", async () => {
    // First reactivate the STO
    await program.methods
      .activateSto()
      .accounts({
        authority: provider.wallet.publicKey,
        stoConfig,
        tokenMint,
      })
      .rpc();

    const investAmount = new anchor.BN(100_000_000); // 100 USDC
    const investor = provider.wallet.publicKey;
    
    // Get investor's token account
    const investorTokenAccount = await getAssociatedTokenAddress(
      tokenMint,
      investor,
      false
    );

    // Get treasury's USDC account
    const treasuryUsdcAccount = await getAssociatedTokenAddress(
      usdcMint,
      provider.wallet.publicKey,
      false
    );

    // Create a lock status account
    const [lockStatus] = await PublicKey.findProgramAddress(
      [Buffer.from("lock_status"), investor.toBuffer(), tokenMint.toBuffer()],
      program.programId
    );

    try {
      await program.methods
        .invest(investAmount, false, true) // amount, is_using_discount, is_accredited
        .accounts({
          investor: investor,
          stoConfig,
          tokenConfig,
          investorUsdcAccount: investorUsdc,
          treasuryUsdcAccount,
          stoTreasury,
          investorTokenAccount,
          lockStatus,
          tokenMint,
          usdcMint,
          tokenProgram: TOKEN_PROGRAM_ID,
          systemProgram: SystemProgram.programId,
          rent: SYSVAR_RENT_PUBKEY,
        })
        .rpc();

      // Verify investment was successful
      const stoConfigAccount = await program.account.stoConfig.fetch(stoConfig);
      expect(stoConfigAccount.totalSold.toNumber()).to.be.greaterThan(0);
      expect(stoConfigAccount.investorCount).to.eq(1);
    } catch (err) {
      console.error("Error making investment:", err);
      throw err;
    }
  });
});