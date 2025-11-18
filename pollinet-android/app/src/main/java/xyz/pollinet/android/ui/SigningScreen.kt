package xyz.pollinet.android.ui

import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.launch
import xyz.pollinet.sdk.*

@Composable
fun SigningScreen(
    sdk: PolliNetSDK?
) {
    val scope = rememberCoroutineScope()
    val keystoreManager = remember { KeystoreManager() }
    
    var signingMethod by remember { mutableStateOf(SigningMethod.KEYSTORE) }
    var unsignedTx by remember { mutableStateOf("") }
    var keyAlias by remember { mutableStateOf("default") }
    var signedTxResult by remember { mutableStateOf<String?>(null) }
    var isSigning by remember { mutableStateOf(false) }
    var errorMessage by remember { mutableStateOf<String?>(null) }
    var keystoreKeys by remember { mutableStateOf<List<String>>(emptyList()) }

    LaunchedEffect(Unit) {
        keystoreKeys = keystoreManager.listKeys()
    }

    Column(
        modifier = Modifier
            .fillMaxSize()
            .padding(16.dp)
            .verticalScroll(rememberScrollState()),
        verticalArrangement = Arrangement.spacedBy(16.dp)
    ) {
        Text(
            text = "Transaction Signing",
            style = MaterialTheme.typography.headlineMedium
        )

        if (sdk == null) {
            Text(
                text = "SDK not initialized",
                color = MaterialTheme.colorScheme.error
            )
            return@Column
        }

        HorizontalDivider()

        // Signing Method Selector
        Card(
            modifier = Modifier.fillMaxWidth(),
            colors = CardDefaults.cardColors(
                containerColor = MaterialTheme.colorScheme.surfaceVariant
            )
        ) {
            Column(modifier = Modifier.padding(16.dp)) {
                Text(
                    text = "Signing Method",
                    style = MaterialTheme.typography.titleMedium,
                    color = MaterialTheme.colorScheme.primary
                )
                Spacer(modifier = Modifier.height(8.dp))
                Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    FilterChip(
                        selected = signingMethod == SigningMethod.KEYSTORE,
                        onClick = { signingMethod = SigningMethod.KEYSTORE },
                        label = { Text("Android Keystore") }
                    )
                    FilterChip(
                        selected = signingMethod == SigningMethod.MWA,
                        onClick = { signingMethod = SigningMethod.MWA },
                        label = { Text("Solana MWA") },
                        enabled = false // TODO: Implement MWA
                    )
                }
                
                if (KeystoreManager.hasStrongBox()) {
                    Spacer(modifier = Modifier.height(4.dp))
                    Text(
                        text = "✓ StrongBox available",
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.primary
                    )
                }
            }
        }

        // Keystore Management
        if (signingMethod == SigningMethod.KEYSTORE) {
            Card(
                modifier = Modifier.fillMaxWidth(),
                colors = CardDefaults.cardColors(
                    containerColor = MaterialTheme.colorScheme.surfaceVariant
                )
            ) {
                Column(
                    modifier = Modifier.padding(16.dp),
                    verticalArrangement = Arrangement.spacedBy(12.dp)
                ) {
                    Text(
                        text = "Keystore Management",
                        style = MaterialTheme.typography.titleMedium,
                        color = MaterialTheme.colorScheme.primary
                    )
                    
                    Text(
                        text = "⚠️ Note: Android Keystore uses ECDSA, not Ed25519. " +
                                "For production use with Solana, prefer Solana Mobile Wallet Adapter.",
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant
                    )
                    
                    OutlinedTextField(
                        value = keyAlias,
                        onValueChange = { keyAlias = it },
                        label = { Text("Key Alias") },
                        modifier = Modifier.fillMaxWidth(),
                        singleLine = true
                    )
                    
                    Row(
                        modifier = Modifier.fillMaxWidth(),
                        horizontalArrangement = Arrangement.spacedBy(8.dp)
                    ) {
                        Button(
                            onClick = {
                                scope.launch {
                                    keystoreManager.generateKeyPair(
                                        alias = keyAlias,
                                        requireUserAuthentication = false,
                                        useStrongBox = true
                                    ).onSuccess {
                                        keystoreKeys = keystoreManager.listKeys()
                                        errorMessage = "Key generated successfully"
                                    }.onFailure {
                                        errorMessage = "Failed to generate key: ${it.message}"
                                    }
                                }
                            },
                            modifier = Modifier.weight(1f)
                        ) {
                            Text("Generate Key")
                        }
                        
                        Button(
                            onClick = {
                                scope.launch {
                                    keystoreManager.deleteKey(keyAlias).onSuccess {
                                        keystoreKeys = keystoreManager.listKeys()
                                        errorMessage = "Key deleted"
                                    }.onFailure {
                                        errorMessage = "Failed to delete key: ${it.message}"
                                    }
                                }
                            },
                            modifier = Modifier.weight(1f),
                            enabled = keystoreManager.keyExists(keyAlias)
                        ) {
                            Text("Delete Key")
                        }
                    }
                    
                    if (keystoreKeys.isNotEmpty()) {
                        Text(
                            text = "Stored Keys: ${keystoreKeys.joinToString(", ")}",
                            style = MaterialTheme.typography.bodySmall
                        )
                    }
                }
            }
        }

        // Transaction Input
        Card(
            modifier = Modifier.fillMaxWidth(),
            colors = CardDefaults.cardColors(
                containerColor = MaterialTheme.colorScheme.surfaceVariant
            )
        ) {
            Column(
                modifier = Modifier.padding(16.dp),
                verticalArrangement = Arrangement.spacedBy(12.dp)
            ) {
                Text(
                    text = "Unsigned Transaction",
                    style = MaterialTheme.typography.titleMedium,
                    color = MaterialTheme.colorScheme.primary
                )
                
                OutlinedTextField(
                    value = unsignedTx,
                    onValueChange = { unsignedTx = it },
                    label = { Text("Base64 Transaction") },
                    modifier = Modifier.fillMaxWidth(),
                    minLines = 3,
                    maxLines = 5
                )
            }
        }

        // Sign Button
        Button(
            onClick = {
                scope.launch {
                    isSigning = true
                    errorMessage = null
                    signedTxResult = null
                    
                    when (signingMethod) {
                        SigningMethod.KEYSTORE -> {
                            // 1. Prepare sign payload
                            val payload = sdk.prepareSignPayload(unsignedTx)
                            if (payload == null) {
                                errorMessage = "Failed to prepare sign payload"
                                isSigning = false
                                return@launch
                            }
                            
                            // 2. Sign with keystore
                            keystoreManager.sign(keyAlias, payload).onSuccess { signature ->
                                // 3. Convert ECDSA signature to Ed25519 format (warning: not cryptographically valid!)
                                val ed25519Sig = SignatureAdapter.ecdsaToEd25519Format(signature)
                                
                                // 4. Get public key
                                keystoreManager.getPublicKey(keyAlias).onSuccess { pubKey ->
                                    // Note: This is a demo - the pubkey format won't match Solana's expectations
                                    // In production, use MWA which properly handles Ed25519
                                    
                                    // 5. Apply signature
                                    sdk.applySignature(
                                        unsignedTx,
                                        pubKey, // This won't be a valid Solana pubkey!
                                        ed25519Sig
                                    ).onSuccess { signedTx ->
                                        signedTxResult = signedTx
                                    }.onFailure {
                                        errorMessage = "Failed to apply signature: ${it.message}"
                                    }
                                }.onFailure {
                                    errorMessage = "Failed to get public key: ${it.message}"
                                }
                            }.onFailure {
                                errorMessage = "Failed to sign: ${it.message}"
                            }
                        }
                        SigningMethod.MWA -> {
                            errorMessage = "MWA not yet implemented"
                        }
                    }
                    
                    isSigning = false
                }
            },
            modifier = Modifier.fillMaxWidth(),
            enabled = !isSigning && unsignedTx.isNotBlank() && 
                    (signingMethod == SigningMethod.KEYSTORE && keystoreManager.keyExists(keyAlias))
        ) {
            if (isSigning) {
                CircularProgressIndicator(
                    modifier = Modifier.size(24.dp),
                    color = MaterialTheme.colorScheme.onPrimary
                )
                Spacer(modifier = Modifier.width(8.dp))
            }
            Text(if (isSigning) "Signing..." else "Sign Transaction")
        }

        // Result Display
        errorMessage?.let { error ->
            Card(
                modifier = Modifier.fillMaxWidth(),
                colors = CardDefaults.cardColors(
                    containerColor = if (error.contains("success")) 
                        MaterialTheme.colorScheme.primaryContainer
                    else
                        MaterialTheme.colorScheme.errorContainer
                )
            ) {
                Column(modifier = Modifier.padding(16.dp)) {
                    Text(
                        text = if (error.contains("success")) "Info" else "Error",
                        style = MaterialTheme.typography.titleMedium,
                        color = if (error.contains("success"))
                            MaterialTheme.colorScheme.primary
                        else
                            MaterialTheme.colorScheme.error
                    )
                    Text(text = error, style = MaterialTheme.typography.bodyMedium)
                }
            }
        }

        signedTxResult?.let { tx ->
            Card(
                modifier = Modifier.fillMaxWidth(),
                colors = CardDefaults.cardColors(
                    containerColor = MaterialTheme.colorScheme.primaryContainer
                )
            ) {
                Column(modifier = Modifier.padding(16.dp)) {
                    Text(
                        text = "✅ Transaction Signed",
                        style = MaterialTheme.typography.titleMedium,
                        color = MaterialTheme.colorScheme.primary
                    )
                    Spacer(modifier = Modifier.height(8.dp))
                    Text(
                        text = "Base64 Length: ${tx.length} bytes",
                        style = MaterialTheme.typography.bodyMedium
                    )
                    Spacer(modifier = Modifier.height(8.dp))
                    Text(
                        text = "Signed Transaction (truncated):",
                        style = MaterialTheme.typography.bodySmall
                    )
                    Text(
                        text = tx.take(100) + "...",
                        style = MaterialTheme.typography.bodySmall,
                        fontFamily = androidx.compose.ui.text.font.FontFamily.Monospace
                    )
                }
            }
        }
    }
}

private enum class SigningMethod {
    KEYSTORE, MWA
}

