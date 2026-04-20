package xyz.pollinet.android.mwa

import android.content.Context
import android.net.Uri
import com.solana.mobilewalletadapter.clientlib.ActivityResultSender
import com.solana.mobilewalletadapter.clientlib.MobileWalletAdapter
import com.solana.mobilewalletadapter.clientlib.TransactionResult
import com.solana.mobilewalletadapter.clientlib.ConnectionIdentity
import com.solana.mobilewalletadapter.clientlib.Solana
import com.solana.publickey.SolanaPublicKey

/**
 * High-level MWA (Mobile Wallet Adapter) client for PolliNet
 * 
 * Handles authorization, signing, and reauthorization flows with Solana Mobile wallets.
 * Integrates seamlessly with PolliNet's unsigned transaction flow.
 * 
 * Based on official MWA SDK documentation:
 * https://docs.solanamobile.com/android-native/using_mobile_wallet_adapter
 * 
 * Core Flow:
 * 1. User authorizes app with their wallet (Solflare, Phantom, etc.)
 * 2. PolliNet creates unsigned transaction using public keys only
 * 3. MWA signs the transaction securely (private keys never leave Seed Vault)
 * 4. PolliNet submits the signed transaction to Solana
 */
class PolliNetMwaClient private constructor(
    private val context: Context,
    private val identityUri: Uri,
    private val iconUri: Uri,
    private val identityName: String
) {
    private val walletAdapter: MobileWalletAdapter
    private var authorizedPublicKey: String? = null
    
    init {
        // Initialize MobileWalletAdapter with connection identity
        // Ref: https://docs.solanamobile.com/android-native/using_mobile_wallet_adapter#instantiate-mobilewalletadapter-client
        walletAdapter = MobileWalletAdapter(
            connectionIdentity = ConnectionIdentity(
                identityUri = identityUri,
                iconUri = iconUri,
                identityName = identityName
            )
        )
        walletAdapter.blockchain = Solana.Devnet
    }
    
    companion object {
        /**
         * Create a new MWA client with app identity
         */
        fun create(
            context: Context,
            identityUri: String,
            iconUri: String,
            identityName: String
        ): PolliNetMwaClient {
            return PolliNetMwaClient(
                context = context,
                identityUri = Uri.parse(identityUri),
                iconUri = Uri.parse(iconUri),
                identityName = identityName
            )
        }
    }
    
    /**
     * Check if user has an authorized session
     */
    fun isAuthorized(): Boolean = authorizedPublicKey != null && walletAdapter.authToken != null
    
    /**
     * Get the currently authorized public key
     */
    fun getAuthorizedPublicKey(): String? = authorizedPublicKey
    
    /**
     * Authorize with a Solana Mobile wallet (Solflare, Phantom, etc.)
     * 
     * This opens the wallet app and requests authorization. The user must approve
     * the connection request.
     * 
     * Uses the `connect()` method from MWA SDK which handles both the association
     * and authorization in one call.
     * 
     * Ref: https://docs.solanamobile.com/android-native/using_mobile_wallet_adapter#connecting-to-a-wallet
     * 
     * @param sender ActivityResultSender for launching the wallet
     * @return Authorized public key (base58)
     * @throws MwaException if authorization fails
     */
    suspend fun authorize(sender: ActivityResultSender): String {
        val result = walletAdapter.connect(sender)
        
        return when (result) {
            is TransactionResult.Success -> {
                // On success, an AuthorizationResult is returned
                val authResult = result.authResult
                
                // Store the authorized public key
                val pubkeyBytes = authResult.accounts.firstOrNull()?.publicKey
                if (pubkeyBytes != null) {
                    // Convert bytes to base58 string using SolanaPublicKey
                    val pubkey = SolanaPublicKey(pubkeyBytes)
                    authorizedPublicKey = pubkey.base58()
                    
                    // authToken is automatically managed by MobileWalletAdapter
                    authorizedPublicKey!!
                } else {
                    throw MwaException("Authorization succeeded but no account returned")
                }
            }
            is TransactionResult.NoWalletFound -> {
                throw MwaException(
                    "No MWA-compatible wallet found on device. " +
                    "Please install Solflare, Phantom, or backpack for testing."
                )
            }
            is TransactionResult.Failure -> {
                throw MwaException(
                    "Authorization failed: ${result.e.message}",
                    result.e
                )
            }
        }
    }
    
    /**
     * Reauthorize with an existing auth token
     * 
     * The MWA SDK automatically manages the authToken and uses it for
     * subsequent requests. If you've persisted the authToken from a previous
     * session, you can restore it here.
     * 
     * Ref: https://docs.solanamobile.com/android-native/using_mobile_wallet_adapter#managing-the-authtoken
     * 
     * @param sender ActivityResultSender for launching the wallet
     * @param cachedAuthToken Previously obtained auth token
     * @return Reauthorized public key (base58)
     * @throws MwaException if reauthorization fails
     */
    suspend fun reauthorize(sender: ActivityResultSender, cachedAuthToken: String): String {
        // Restore the cached authToken
        walletAdapter.authToken = cachedAuthToken
        
        // Attempt to connect - if token is valid, user won't need to approve again
        val result = walletAdapter.connect(sender)
        
        return when (result) {
            is TransactionResult.Success -> {
                val authResult = result.authResult
                val pubkeyBytes = authResult.accounts.firstOrNull()?.publicKey
                if (pubkeyBytes != null) {
                    val pubkey = SolanaPublicKey(pubkeyBytes)
                    authorizedPublicKey = pubkey.base58()
                    authorizedPublicKey!!
                } else {
                    throw MwaException("Reauthorization succeeded but no account returned")
                }
            }
            is TransactionResult.NoWalletFound -> {
                throw MwaException("No MWA-compatible wallet found on device")
            }
            is TransactionResult.Failure -> {
                // Token may have expired, need full authorization
                throw MwaException(
                    "Reauthorization failed: ${result.e.message}. " +
                    "User needs to authorize again.",
                    result.e
                )
            }
        }
    }
    
    /**
     * Sign a transaction using MWA
     * 
     * This takes an unsigned transaction from PolliNet's `createUnsignedOfflineTransaction`,
     * sends it to the wallet for signing, and returns the signed transaction.
     * 
     * Uses the `signTransactions` method (deprecated but still supported for backwards compatibility).
     * The transaction is signed but NOT sent - PolliNet will submit it to the network.
     * 
     * The private keys NEVER leave the Seed Vault - signing happens in secure hardware.
     * 
     * Ref: https://docs.solanamobile.com/android-native/using_mobile_wallet_adapter#signing-transactions-deprecated
     * 
     * @param sender ActivityResultSender for launching the wallet
     * @param unsignedTransactionBase64 Base64-encoded unsigned transaction from PolliNet SDK
     * @return Signed transaction bytes
     * @throws MwaException if signing fails or user rejects
     */
    suspend fun signAndSendTransaction(
        sender: ActivityResultSender,
        unsignedTransactionBase64: String
    ): ByteArray {
        if (!isAuthorized()) {
            throw MwaException("Not authorized. Call authorize() first.")
        }
        
        // Decode the unsigned transaction from base64
        val txBytes = android.util.Base64.decode(
            unsignedTransactionBase64,
            android.util.Base64.NO_WRAP
        )
        
        // Use transact to establish session and sign transaction
        val result = walletAdapter.transact(sender) { authResult ->
            // Sign the transaction (deprecated method but still supported)
            // Note: We use signTransactions instead of signAndSendTransactions
            // because PolliNet handles the submission to the network
            signTransactions(arrayOf(txBytes))
        }
        
        when (result) {
            is TransactionResult.Success -> {
                // Extract the signed transaction payload
                // The payload contains the signed transactions
                val payload = result.payload
                if (payload is Array<*> && payload.isNotEmpty()) {
                    val signedTxBytes = payload[0] as? ByteArray
                    return if (signedTxBytes != null) {
                        signedTxBytes
                    } else {
                        throw MwaException("Wallet returned invalid signed transaction format")
                    }
                } else {
                    throw MwaException("Wallet returned success but no signed transaction")
                }
            }
            is TransactionResult.NoWalletFound -> {
                throw MwaException("No MWA-compatible wallet found on device")
            }
            is TransactionResult.Failure -> {
                // User rejected or signing failed
                throw MwaException(
                    "Transaction signing failed: ${result.e.message}. " +
                    "User may have rejected the request.",
                    result.e
                )
            }
        }
    }
    
    /**
     * Sign an unsigned transaction with wallet
     * 
     * Use this for transactions where the wallet needs to add the payer signature.
     * Returns the transaction with the payer signature added (may still need additional signatures).
     * 
     * @param sender ActivityResultSender for launching the wallet
     * @param unsignedTxBase64 Base64-encoded unsigned transaction
     * @return Transaction bytes with payer signature added
     */
    suspend fun signTransaction(
        sender: ActivityResultSender,
        unsignedTxBase64: String
    ): ByteArray {
        if (!isAuthorized()) {
            throw MwaException("Not authorized. Call authorize() first.")
        }
        
        android.util.Log.d("PolliNetMwaClient", "Signing unsigned transaction with wallet")
        
        val txBytes = android.util.Base64.decode(
            unsignedTxBase64,
            android.util.Base64.NO_WRAP
        )
        
        // Use signTransactions to add payer signature
        val result = walletAdapter.transact(sender) {
            android.util.Log.d("PolliNetMwaClient", "Requesting payer signature from wallet...")
            signTransactions(arrayOf(txBytes))
        }
        
        when (result) {
            is TransactionResult.Success -> {
                val payload = result.payload
                android.util.Log.d("PolliNetMwaClient", "✅ Wallet signed successfully")
                android.util.Log.d("PolliNetMwaClient", "Payload type: ${payload?.javaClass?.name}")
                
                if (payload == null) {
                    throw MwaException("Wallet returned null payload")
                }
                
                // MWA returns SignPayloadsResult which wraps the actual signed transactions
                // Access the signedPayloads field to get Array<ByteArray>
                val signedPayloads = try {
                    // Use reflection to access signedPayloads field from SignPayloadsResult
                    val field = payload.javaClass.getDeclaredField("signedPayloads")
                    field.isAccessible = true
                    field.get(payload) as? Array<ByteArray>
                } catch (e: Exception) {
                    android.util.Log.e("PolliNetMwaClient", "Failed to extract signedPayloads: ${e.message}")
                    null
                }
                
                if (signedPayloads != null && signedPayloads.isNotEmpty()) {
                    val signedTxBytes = signedPayloads[0]
                    android.util.Log.d("PolliNetMwaClient", "✅ Got signed transaction (${signedTxBytes.size} bytes)")
                    return signedTxBytes
                }
                
                throw MwaException("Failed to extract signed transaction from payload")
            }
            is TransactionResult.NoWalletFound -> {
                throw MwaException("No MWA-compatible wallet found on device")
            }
            is TransactionResult.Failure -> {
                throw MwaException(
                    "Signing failed: ${result.e.message}",
                    result.e
                )
            }
        }
    }

    /**
     * Sign raw message bytes with the wallet's Ed25519 key.
     *
     * Used to sign Pollinet intent bytes (169 bytes) before submitting to pollicore.
     * The wallet signs the bytes directly; the returned value is the 64-byte signature.
     *
     * @param sender ActivityResultSender for launching the wallet.
     * @param messageBytes Raw bytes to sign.
     * @return 64-byte Ed25519 signature.
     */
    suspend fun signMessage(
        sender: ActivityResultSender,
        messageBytes: ByteArray,
    ): ByteArray {
        if (!isAuthorized()) {
            throw MwaException("Not authorized. Call authorize() first.")
        }

        val pubkeyBase58 = authorizedPublicKey ?: throw MwaException("No authorized public key")
        // Encode pubkey as bytes for the addresses parameter
        val pubkeyBytes = bs58Decode(pubkeyBase58)

        val result = walletAdapter.transact(sender) {
            signMessages(
                messages = arrayOf(messageBytes),
                addresses = arrayOf(pubkeyBytes),
            )
        }

        return when (result) {
            is TransactionResult.Success -> {
                val payload = result.payload
                val signedPayloads = try {
                    val field = payload.javaClass.getDeclaredField("signedPayloads")
                    field.isAccessible = true
                    field.get(payload) as? Array<ByteArray>
                } catch (e: Exception) {
                    android.util.Log.e("PolliNetMwaClient", "signMessage: failed to extract: ${e.message}")
                    null
                }

                val signed = signedPayloads?.firstOrNull()
                    ?: throw MwaException("Wallet returned no signed message payload")

                // Wallets return the 64-byte signature (may be prefixed in some implementations)
                if (signed.size >= 64) signed.take(64).toByteArray()
                else throw MwaException("Signed payload is too short: ${signed.size} bytes")
            }
            is TransactionResult.NoWalletFound ->
                throw MwaException("No MWA-compatible wallet found on device")
            is TransactionResult.Failure ->
                throw MwaException("Message signing failed: ${result.e.message}", result.e)
        }
    }

    // ─── Helpers ─────────────────────────────────────────────────────────────

    private fun bs58Decode(input: String): ByteArray {
        val alphabet = "123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz"
        val bytes = mutableListOf<Int>(0)
        for (char in input) {
            val v = alphabet.indexOf(char)
            if (v < 0) throw MwaException("Invalid base58 character: $char")
            var carry = v
            for (i in bytes.indices) {
                carry += bytes[i] * 58
                bytes[i] = carry and 0xff
                carry = carry shr 8
            }
            while (carry > 0) { bytes.add(carry and 0xff); carry = carry shr 8 }
        }
        for (char in input) { if (char != '1') break; bytes.add(0) }
        return bytes.reversed().map { it.toByte() }.toByteArray()
    }

    /**
     * Get auth token for persistent storage
     * 
     * Save this token to SharedPreferences or secure storage to avoid
     * asking the user to reauthorize on every app launch.
     * 
     * The authToken is automatically managed by MobileWalletAdapter and
     * updated after successful connections.
     * 
     * Ref: https://docs.solanamobile.com/android-native/using_mobile_wallet_adapter#managing-the-authtoken
     */
    fun getAuthToken(): String? = walletAdapter.authToken
    
    /**
     * Deauthorize and clear cached credentials
     * 
     * This invalidates the current session and clears the authToken.
     * The user will need to authorize again for future requests.
     * 
     * Ref: https://docs.solanamobile.com/android-native/using_mobile_wallet_adapter#disconnecting-from-a-wallet
     */
    suspend fun deauthorize(sender: ActivityResultSender) {
        if (walletAdapter.authToken != null) {
            val result = walletAdapter.disconnect(sender)
            
            when (result) {
                is TransactionResult.Success -> {
                    // Successfully invalidated the authToken
                    authorizedPublicKey = null
                }
                is TransactionResult.NoWalletFound -> {
                    // Wallet not found, clear local state anyway
                    authorizedPublicKey = null
                }
                is TransactionResult.Failure -> {
                    // Failed to disconnect, clear local state anyway
                    authorizedPublicKey = null
                }
            }
        } else {
            // No active session, just clear local state
            authorizedPublicKey = null
        }
    }
    
    /**
     * Clear local session state without contacting the wallet
     * 
     * Use this when you want to clear the session locally without
     * invalidating the authToken on the wallet side.
     */
    fun clearLocalSession() {
        authorizedPublicKey = null
        walletAdapter.authToken = null
    }
}

/**
 * Exception thrown by MWA operations
 */
class MwaException(
    message: String,
    cause: Throwable? = null
) : Exception(message, cause)
