# Web Search API Keys Setup

This document explains how to configure API keys for Tavily and Exa web search providers.

## API Keys Provided

- **Tavily API Key**: `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`
- **Exa API Key**: `92083e68-6aa1-4eaa-b8fe-b62855000a2c`

## Setup Methods

The application supports two methods for storing API keys:

1. **Environment Variables** (Recommended for development)
2. **System Keychain** (Recommended for production)

The application checks environment variables first, then falls back to the system keychain.

---

## Method 1: Environment Variables (Recommended - Already Set Up!)

✅ **The API keys have been automatically added to your `~/.zshrc` file!**

The keys are now permanently configured. They will be available:
- In all new terminal sessions
- When you launch the application from the terminal
- When the application reads environment variables

### To Apply the Changes

**Option 1: Reload your current shell**
```bash
source ~/.zshrc
```

**Option 2: Open a new terminal window**
The keys will be automatically loaded.

### Verify the Keys are Set

```bash
echo $TAVILY_API_KEY
echo $EXA_API_KEY
```

### Manual Setup (if needed)

If you need to add them manually, add these lines to your shell profile:

**For zsh** (`~/.zshrc`):
```bash
export TAVILY_API_KEY="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"
export EXA_API_KEY="92083e68-6aa1-4eaa-b8fe-b62855000a2c"
```

**For bash** (`~/.bashrc`):
```bash
export TAVILY_API_KEY="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"
export EXA_API_KEY="92083e68-6aa1-4eaa-b8fe-b62855000a2c"
```

Then reload:
```bash
source ~/.zshrc  # or source ~/.bashrc
```

### Option C: Per-Session Setup

For a single terminal session:

```bash
export TAVILY_API_KEY="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"
export EXA_API_KEY="92083e68-6aa1-4eaa-b8fe-b62855000a2c"
```

### Verify Environment Variables

Check if the keys are set:

```bash
echo $TAVILY_API_KEY
echo $EXA_API_KEY
```

---

## Method 2: System Keychain (Production)

The application can store API keys in the system keychain using the `store_api_key` function. This is more secure for production use.

### Keychain Storage Details

- **Tavily Keychain URL**: `web_search_provider://tavily`
- **Exa Keychain URL**: `web_search_provider://exa`

The keys are stored securely in your system's keychain (macOS Keychain, Windows Credential Manager, or Linux secret service).

### Using Keychain Storage

The keychain storage is handled automatically by the application when environment variables are not set. You can also programmatically store keys using the `store_api_key` function from `crates/web_search_providers/src/api_key.rs`.

---

## How the Application Loads Keys

The application uses the following priority order:

1. **Environment Variables** (`TAVILY_API_KEY`, `EXA_API_KEY`)
2. **System Keychain** (fallback)

The loading logic is in `crates/web_search_providers/src/api_key.rs`:

```rust
pub async fn load_api_key(
    provider_name: &str,
    env_var_name: &str,
    cx: &AsyncApp,
) -> Result<Option<Arc<str>>> {
    // 1. Check environment variable first
    // 2. Fall back to system keychain
}
```

---

## Verification

### Check if Keys are Loaded

The application will automatically register providers if API keys are available. You can verify this by:

1. **Running the application** - Providers will be registered on startup if keys are found
2. **Checking logs** - Look for provider registration messages
3. **Testing web search** - Try using the web search tool in the application

### Test API Keys

You can test the API keys using the integration tests:

```bash
# Test Tavily API
cargo test --package agent --lib -- --ignored test_tavily_real_api_search --nocapture

# Test Exa API
cargo test --package agent --lib -- --ignored test_exa_real_api_search --nocapture

# Test both
cargo test --package agent --lib -- --ignored real_api --nocapture
```

---

## Security Notes

⚠️ **Important Security Considerations**:

1. **Never commit API keys to version control**
   - The keys are currently hardcoded in test files (`crates/agent/src/tests/web_search_real_api_tests.rs`)
   - This is acceptable for tests, but ensure they're not in production code

2. **Use environment variables for development**
   - Easy to set up and change
   - Not persisted across system restarts (unless added to shell profile)

3. **Use keychain for production**
   - More secure
   - Persisted across system restarts
   - Encrypted by the operating system

4. **Rotate keys if compromised**
   - If keys are exposed, regenerate them from the provider dashboards:
     - Tavily: https://tavily.com/dashboard
     - Exa: https://dashboard.exa.ai/

---

## Troubleshooting

### Keys Not Loading

1. **Check environment variables are set**:
   ```bash
   echo $TAVILY_API_KEY
   echo $EXA_API_KEY
   ```

2. **Verify the application can read them**:
   - Restart the application after setting environment variables
   - Check application logs for provider registration

3. **Check keychain access**:
   - On macOS, ensure the application has keychain access permissions
   - On Linux, ensure secret service is running

### Providers Not Registering

If providers are not registering:

1. Verify API keys are correct
2. Check network connectivity
3. Verify API keys are not expired
4. Check application logs for error messages

---

## API Key Locations in Code

- **Test files** (hardcoded for testing):
  - `crates/agent/src/tests/web_search_real_api_tests.rs` (lines 12-13)

- **Production code** (loads from env/keychain):
  - `crates/web_search_providers/src/api_key.rs` - Key loading logic
  - `crates/web_search_providers/src/web_search_providers.rs` - Provider registration

---

## Next Steps

1. **Set up environment variables** using one of the methods above
2. **Restart the application** to load the keys
3. **Test web search** functionality
4. **Consider keychain storage** for production deployments

---

## Support

If you encounter issues with API key setup:

1. Check the application logs
2. Verify environment variables are set correctly
3. Test API keys directly with the providers' APIs
4. Review the troubleshooting section above

