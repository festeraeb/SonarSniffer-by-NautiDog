# USGS M2M API Update

## Summary

Updated `universal_downloader.py` to use the new USGS Earth Explorer Machine-to-Machine (M2M) JSON API authentication method.

## Background

On February 26, 2025, USGS deprecated the legacy username/password login endpoint for the M2M API. Users must now authenticate using an application token obtained from their ERS profile.

## Changes Made

### USGSDownloader Class (`universal_downloader.py`)

**Old Authentication Flow:**
- Used `X-Auth-Token` header with the API key directly
- No explicit login call required

**New Authentication Flow:**
1. Call `login-token` endpoint with the application token
2. Receive a session token in response
3. Use the session token for all subsequent API calls

### Key Code Changes

1. **Added `_login()` method:**
   - Calls `https://m2m.cr.usgs.gov/api/api/json/stable/login-token`
   - Sends application token in payload: `{"token": "<api_key>"}`
   - Stores session token for reuse
   - Handles authentication errors gracefully

2. **Updated `_call()` method:**
   - Now calls `_login()` before each API request
   - Uses session token in `X-Auth-Token` header
   - Maintains backward compatibility with error handling

3. **Configuration:**
   - Reads from `USGS_API_KEY` environment variable
   - This should contain the 64-bit encrypted application token from ERS profile

## How to Configure

1. Log in to [EarthExplorer](https://earthexplorer.usgs.gov/)
2. Go to Profile → Application Tokens
3. Create a new application token (or copy existing one)
4. Add to your `.env` or `credentials.sh`:

```bash
USGS_API_KEY=your_64_bit_encrypted_token_here
```

## Testing

To test the updated downloader:

```bash
python universal_downloader.py --area "straits_of_mackinac" --dates 2024-06-01 2024-09-30 --sensors landsat
```

Or for a dry run:

```bash
python universal_downloader.py --dry-run --area "straits_of_mackinac" --dates 2024-06-01 2024-09-30 --sensors landsat
```

## References

- [USGS M2M API Documentation](https://m2m.cr.usgs.gov/api/docs/json/)
- [M2M Application Token Documentation](https://www.usgs.gov/media/files/m2m-application-token-documentation)

## Notes

- The application token is a 64-bit encrypted string (not a plain API key)
- Tokens can be revoked/rotated from the ERS profile without changing credentials
- Session tokens are stored in memory and reused for the session lifetime
