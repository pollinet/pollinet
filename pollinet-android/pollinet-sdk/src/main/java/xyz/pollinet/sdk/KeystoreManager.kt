package xyz.pollinet.sdk

import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyProperties
import android.util.Base64
import java.security.*
import java.security.spec.ECGenParameterSpec
import javax.crypto.KeyAgreement

/**
 * Android Keystore manager for generating and using Ed25519/EC keys.
 * 
 * Note: Ed25519 is not directly supported in Android Keystore, so we use
 * ECDSA (secp256r1) as a fallback for secure key storage.
 * 
 * For production use with Solana, you should prefer Solana Mobile Wallet Adapter (MWA)
 * which properly supports Ed25519. This is a fallback for devices without MWA.
 */
class KeystoreManager {
    companion object {
        private const val ANDROID_KEYSTORE = "AndroidKeyStore"
        private const val KEY_ALIAS_PREFIX = "pollinet_"
        
        /**
         * Check if device has StrongBox support
         */
        fun hasStrongBox(): Boolean {
            return try {
                val keyStore = KeyStore.getInstance(ANDROID_KEYSTORE)
                keyStore.load(null)
                // Try to detect StrongBox by checking key properties
                // This is a best-effort check
                android.os.Build.VERSION.SDK_INT >= android.os.Build.VERSION_CODES.P
            } catch (e: Exception) {
                false
            }
        }
    }

    private val keyStore: KeyStore = KeyStore.getInstance(ANDROID_KEYSTORE).apply {
        load(null)
    }

    /**
     * Generate a new ECDSA key pair in Android Keystore
     * 
     * @param alias Unique identifier for the key
     * @param requireUserAuthentication Whether to require biometric/PIN for key use
     * @param useStrongBox Whether to use StrongBox if available
     * @return Public key as base64-encoded bytes
     */
    fun generateKeyPair(
        alias: String,
        requireUserAuthentication: Boolean = false,
        useStrongBox: Boolean = true
    ): Result<String> {
        return try {
            val keyAlias = KEY_ALIAS_PREFIX + alias
            
            val spec = KeyGenParameterSpec.Builder(
                keyAlias,
                KeyProperties.PURPOSE_SIGN or KeyProperties.PURPOSE_VERIFY
            ).apply {
                setAlgorithmParameterSpec(ECGenParameterSpec("secp256r1"))
                setDigests(KeyProperties.DIGEST_SHA256)
                
                // User authentication
                if (requireUserAuthentication) {
                    setUserAuthenticationRequired(true)
                    if (android.os.Build.VERSION.SDK_INT >= android.os.Build.VERSION_CODES.R) {
                        setUserAuthenticationParameters(
                            30, // timeout seconds
                            KeyProperties.AUTH_BIOMETRIC_STRONG or KeyProperties.AUTH_DEVICE_CREDENTIAL
                        )
                    }
                }
                
                // StrongBox (hardware-backed if available)
                if (useStrongBox && android.os.Build.VERSION.SDK_INT >= android.os.Build.VERSION_CODES.P) {
                    setIsStrongBoxBacked(true)
                }
            }.build()

            val keyPairGenerator = KeyPairGenerator.getInstance(
                KeyProperties.KEY_ALGORITHM_EC,
                ANDROID_KEYSTORE
            )
            keyPairGenerator.initialize(spec)
            val keyPair = keyPairGenerator.generateKeyPair()
            
            // Return public key as base64
            val publicKeyBytes = keyPair.public.encoded
            val base64PublicKey = Base64.encodeToString(publicKeyBytes, Base64.NO_WRAP)
            
            Result.success(base64PublicKey)
        } catch (e: Exception) {
            Result.failure(e)
        }
    }

    /**
     * Sign data with a stored key
     * 
     * @param alias Key identifier
     * @param data Data to sign
     * @return Signature bytes
     */
    fun sign(alias: String, data: ByteArray): Result<ByteArray> {
        return try {
            val keyAlias = KEY_ALIAS_PREFIX + alias
            
            val entry = keyStore.getEntry(keyAlias, null) as? KeyStore.PrivateKeyEntry
                ?: return Result.failure(Exception("Key not found: $alias"))

            val signature = Signature.getInstance("SHA256withECDSA")
            signature.initSign(entry.privateKey)
            signature.update(data)
            val signatureBytes = signature.sign()
            
            Result.success(signatureBytes)
        } catch (e: Exception) {
            Result.failure(e)
        }
    }

    /**
     * Get public key for a stored key
     * 
     * @param alias Key identifier
     * @return Public key as base64-encoded bytes
     */
    fun getPublicKey(alias: String): Result<String> {
        return try {
            val keyAlias = KEY_ALIAS_PREFIX + alias
            
            val entry = keyStore.getEntry(keyAlias, null) as? KeyStore.PrivateKeyEntry
                ?: return Result.failure(Exception("Key not found: $alias"))

            val publicKeyBytes = entry.certificate.publicKey.encoded
            val base64PublicKey = Base64.encodeToString(publicKeyBytes, Base64.NO_WRAP)
            
            Result.success(base64PublicKey)
        } catch (e: Exception) {
            Result.failure(e)
        }
    }

    /**
     * Delete a key from the keystore
     * 
     * @param alias Key identifier
     */
    fun deleteKey(alias: String): Result<Unit> {
        return try {
            val keyAlias = KEY_ALIAS_PREFIX + alias
            keyStore.deleteEntry(keyAlias)
            Result.success(Unit)
        } catch (e: Exception) {
            Result.failure(e)
        }
    }

    /**
     * Check if a key exists
     * 
     * @param alias Key identifier
     * @return true if key exists
     */
    fun keyExists(alias: String): Boolean {
        val keyAlias = KEY_ALIAS_PREFIX + alias
        return keyStore.containsAlias(keyAlias)
    }

    /**
     * List all PolliNet keys
     * 
     * @return List of key aliases (without prefix)
     */
    fun listKeys(): List<String> {
        return keyStore.aliases().toList()
            .filter { it.startsWith(KEY_ALIAS_PREFIX) }
            .map { it.removePrefix(KEY_ALIAS_PREFIX) }
    }
}

/**
 * Simple ECDSA to Ed25519-compatible signature adapter
 * 
 * WARNING: This is NOT cryptographically equivalent to Ed25519!
 * This is a fallback for demonstration purposes only.
 * 
 * For production Solana apps, use Solana Mobile Wallet Adapter (MWA)
 * which properly supports Ed25519 signatures.
 */
object SignatureAdapter {
    /**
     * Convert ECDSA signature to 64-byte format (padding/truncation)
     * 
     * This is NOT a proper conversion - it's just reshaping the bytes
     * to match the expected signature length.
     */
    fun ecdsaToEd25519Format(ecdsaSignature: ByteArray): ByteArray {
        // Ed25519 signatures are 64 bytes
        // ECDSA signatures vary in length
        // This is a naive approach - just pad or truncate
        
        return when {
            ecdsaSignature.size >= 64 -> {
                // Truncate to 64 bytes
                ecdsaSignature.copyOf(64)
            }
            else -> {
                // Pad with zeros to 64 bytes
                ByteArray(64).apply {
                    ecdsaSignature.copyInto(this, 0)
                }
            }
        }
    }
}

