import * as anchor from "@coral-xyz/anchor";
import { Program, Idl  } from "@coral-xyz/anchor";
import { EstateProtocol, IDL } from "../target/types/estate_protocol";
import { PublicKey, Keypair, SystemProgram } from "@solana/web3.js";
import { TOKEN_PROGRAM_ID } from "@solana/spl-token";
import { expect } from "chai";

describe("estate_protocol", () => {
  const provider = anchor.AnchorProvider.env();
  anchor.setProvider(provider);

  const program = anchor.workspace.EstateProtocol as unknown as Program<EstateProtocol>;

  let mint: Keypair;
  let tokenConfig: PublicKey;
  let configBump: number;

  beforeEach(async () => {
    mint = Keypair.generate();
    
    // Derive PDA for token config
    const [tokenConfigPDA, bump] = await PublicKey.findProgramAddress(
      [
        Buffer.from("token_config"),
        mint.publicKey.toBuffer(),
      ],
      program.programId
    );
    tokenConfig = tokenConfigPDA;
    configBump = bump;
  });

  it("Creates a security token", async () => {
    const tokenName = "Estate Token";
    const tokenSymbol = "EST";
    const tokenDetails = "https://example.com/token";
    const divisible = true;
    const treasuryWallet = provider.wallet.publicKey;
    const documentHash = "QmHash..."; // Example IPFS hash

    try {
      await program.methods
        .createSecurityToken(
          tokenName,
          tokenSymbol,
          tokenDetails,
          divisible,
          treasuryWallet,
          documentHash
        )
        .accounts({
          authority: provider.wallet.publicKey,
          mint: mint.publicKey,
          metadata: await getMetadataAddress(mint.publicKey),
          tokenConfig,
          tokenProgram: TOKEN_PROGRAM_ID,
          tokenMetadataProgram: METADATA_PROGRAM_ID, // Need to add constant
          systemProgram: SystemProgram.programId,
          rent: anchor.web3.SYSVAR_RENT_PUBKEY,
        })
        .signers([mint])
        .rpc();

      // Fetch the token config account
      const tokenConfigAccount = await (program.account as any).tokenConfig.fetch(tokenConfig);

      // Verify the token config data
      expect(tokenConfigAccount.authority).to.eql(provider.wallet.publicKey);
      expect(tokenConfigAccount.mint).to.eql(mint.publicKey);
      expect(tokenConfigAccount.name).to.eq(tokenName);
      expect(tokenConfigAccount.symbol).to.eq(tokenSymbol);
      expect(tokenConfigAccount.decimals).to.eq(9); // Since divisible is true
      expect(tokenConfigAccount.tokenDetails).to.eq(tokenDetails);
      expect(tokenConfigAccount.documentHash).to.eq(documentHash);
      expect(tokenConfigAccount.treasuryWallet).to.eql(treasuryWallet);
      expect(tokenConfigAccount.status).to.eql({ created: {} });
      expect(tokenConfigAccount.bump).to.eq(configBump);

    } catch (err) {
      console.log("Error: ", err);
      throw err;
    }
  });

  it("Validates empty name", async () => {
    try {
      await program.methods
        .createSecurityToken(
          "", // Empty name
          "EST",
          "https://example.com/token",
          true,
          provider.wallet.publicKey,
          "QmHash..."
        )
        .accounts({
          authority: provider.wallet.publicKey,
          mint: mint.publicKey,
          metadata: await getMetadataAddress(mint.publicKey),
          tokenConfig,
          tokenProgram: TOKEN_PROGRAM_ID,
          tokenMetadataProgram: METADATA_PROGRAM_ID,
          systemProgram: SystemProgram.programId,
          rent: anchor.web3.SYSVAR_RENT_PUBKEY,
        })
        .signers([mint])
        .rpc();
      
      expect.fail("Should have failed with empty name");
    } catch (err) {
      expect(err.toString()).to.include("Name cannot be empty");
    }
  });
});

// Helper function to get metadata address
async function getMetadataAddress(mint: PublicKey): Promise<PublicKey> {
  const [metadataAddress] = await PublicKey.findProgramAddress(
    [
      Buffer.from("metadata"),
      METADATA_PROGRAM_ID.toBuffer(),
      mint.toBuffer(),
    ],
    METADATA_PROGRAM_ID
  );
  return metadataAddress;
}

// Add METADATA_PROGRAM_ID constant
const METADATA_PROGRAM_ID = new PublicKey("metaqbxxUerdq28cj1RbAWkYQm3ybzjb6a8bt518x1s");