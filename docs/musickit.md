# Setting up Apple Music (MusicKit)

Encore runs without any setup on Apple's public APIs: charts, the iTunes Search API, and 30-second previews. Some features need the official Apple Music API, which requires a **MusicKit key** from an Apple Developer account:

- signing in to your library
- full-length playback
- the songs inside Apple's editorial playlists
- artist artwork
- personal recommendations on Home

The key belongs to you and never leaves your Mac. Encore uses it to sign a short-lived developer token locally.

## What you need

- A paid [Apple Developer Program](https://developer.apple.com/programs/) membership.
- An Apple Music subscription, if you want full songs. Without one, signed-in playback falls back to previews.

## 1. Create a Media ID

1. Open [Certificates, Identifiers & Profiles → Identifiers](https://developer.apple.com/account/resources/identifiers/list).
2. Click **+**, choose **Media IDs**, and continue.
3. Enter a description, for example "Encore", and an identifier such as `media.com.yourname.encore`.
4. Enable **MusicKit**, then click **Continue** and **Register**.

## 2. Create a MusicKit key

1. Open [Keys](https://developer.apple.com/account/resources/authkeys/list) and click **+**.
2. Name the key, enable **Media Services (MusicKit, ShazamKit, Apple Music Feed)**, click **Configure**, and pick the Media ID from step 1.
3. Click **Continue**, then **Register**, then **Download**. You get a file named `AuthKey_<KEY_ID>.p8`.

   > Apple only lets you download the key once. Keep it somewhere safe, outside this repository.

4. Note the **Key ID**, which is shown on the key's page, and your **Team ID**, which is in the top-right corner of the developer site or under [Membership details](https://developer.apple.com/account#MembershipDetailsCard).

## 3. Point Encore at the key

The simplest way is a config file at `~/Library/Application Support/Encore/musickit.json`:

```json
{
  "team_id": "ABCDE12345",
  "key_id": "XYZ9876543",
  "private_key_path": "/Users/you/keys/AuthKey_XYZ9876543.p8"
}
```

You can use environment variables instead, which is handy with `cargo run`. Encore checks for configuration in this order and uses the first one it finds:

```sh
# 1. A developer token you've already signed
export APPLE_MUSIC_DEVELOPER_TOKEN=eyJ...

# 2. The key itself (Encore signs an ES256 token for you)
export APPLE_MUSIC_TEAM_ID=ABCDE12345
export APPLE_MUSIC_KEY_ID=XYZ9876543
export APPLE_MUSIC_PRIVATE_KEY_PATH=~/keys/AuthKey_XYZ9876543.p8

# 3. Otherwise: ~/Library/Application Support/Encore/musickit.json
```

Restart Encore after changing the configuration. If Encore can't read the key, it prints the reason to stderr at launch and shows it when you click **Sign In**.

## 4. Sign in

Click **Sign In** at the bottom of the sidebar:

1. Encore starts a one-page sign-in site on `127.0.0.1` and opens it in your browser.
2. **Sign In with Apple Music** runs Apple's own MusicKit sign-in. Your Apple ID password goes only to Apple.
3. The page posts the resulting **Music User Token** back to Encore. The page only accepts the token with a random per-session value, so other pages can't hand Encore a token.
4. Encore saves the token in your Keychain under the service `Encore`.

The first time Encore reads the Keychain, macOS asks for permission. Click **Always Allow**. If you build with `./scripts/build.sh` and an Apple Development certificate, the app keeps the same identity between builds, so you won't be asked again.

To sign out, click your name in the sidebar and choose **Sign Out**.

## Troubleshooting

| Symptom | What to check |
| --- | --- |
| **Sign In** says a key is needed | Encore couldn't find or read a key. Check the path in `musickit.json`. `~` isn't expanded there, so use an absolute path. |
| `invalid MusicKit .p8 key` | The file isn't the `.p8` downloaded from Apple, or it has been edited. |
| Songs play as 30-second previews after signing in | The Apple ID needs an active Apple Music subscription. Run Encore from a terminal (`~/Applications/Encore.app/Contents/MacOS/Encore`) and look for `musickit:` or `full playback unavailable` lines. |
| The Keychain prompt comes back after every build | The build isn't signed with a stable identity. Use `./scripts/build.sh` with an Apple Development certificate. |

## Keeping your key private

- Never commit the `.p8` file, `musickit.json`, or a developer token. `.gitignore` already excludes `*.p8` and `musickit.json`.
- Encore signs a fresh developer token at each launch, valid for 30 days.
- If a key leaks, revoke it under **Keys** on the developer site and create a new one.
