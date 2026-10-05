# Courtpit — shipping a community's app

How one community gets its own app in the App Store and Google Play (spec §16, build step 7).
The code is the same for every community; what differs is an EAS build profile in
`apps/mobile/eas.json`, a few accounts, and some server settings. `demo` is the worked example.

Everything below runs from `apps/mobile` unless it says otherwise. You need the EAS CLI
(`npm install -g eas-cli`) logged in to the Expo account that will own the app.

## 1. Accounts the community holds

Apple's guideline 4.3 (template apps) means each community publishes under **its own** Apple
Developer account, not a shared Courtpit one (spec §16).

| What | Who creates it | Notes |
|---|---|---|
| Apple Developer Program | the community (an organisation needs a D-U-N-S number) | Note the **Team ID** (10 characters). |
| Google Play Console | the community | A one-time fee; create the app listing once the first build exists. |
| Expo account or organisation | Courtpit, with the community's Apple/Google access shared to it | Holds the EAS project, builds, credentials and updates. |

## 2. The EAS project

```sh
npx eas init        # creates the project under the logged-in account; prints its id
```

`app.config.ts` is dynamic, so `eas init` can't write the id itself: put it in the profile as
`COURTPIT_EAS_PROJECT_ID` (next step). The id is what push tokens are issued for and where EAS
Update serves updates; a build without it has neither.

## 3. The build profile

Copy the `demo` and `demo-preview` profiles in `eas.json` and change every value:

| Variable | Example | Meaning |
|---|---|---|
| `EXPO_PUBLIC_COMMUNITY` | `riverside` | The community's slug on the server. |
| `EXPO_PUBLIC_API_URL` | `https://riverside.courtpit.app` | Where the app talks to; `https://` + the web host. |
| `COURTPIT_WEB_HOST` | `riverside.courtpit.app` | Its links open the app (step 7). |
| `COURTPIT_APP_NAME` | `Riverside Tennis` | The name under the icon and in the stores. |
| `COURTPIT_BUNDLE_ID` | `app.courtpit.riverside` | iOS bundle id and Android package. **Never change it after release.** |
| `COURTPIT_SCHEME` | `courtpit-riverside` | The app's own URL scheme; unique per community. |
| `COURTPIT_EAS_PROJECT_ID` | from step 2 | Push and updates. |
| `COURTPIT_ICON` | `./assets/communities/riverside/icon.png` | 1024×1024 PNG, no transparency (optional). |
| `EXPO_PUBLIC_GOOGLE_WEB_CLIENT_ID`, `EXPO_PUBLIC_GOOGLE_IOS_CLIENT_ID`, `COURTPIT_GOOGLE_IOS_URL_SCHEME` | from step 6 | Only if the community offers Google sign-in. |

The profile's name is also its EAS Update **channel**. `src/eas.test.ts` (run by `npm test` and
CI) fails if a store profile misses an identity variable, uses a channel other than its name,
points the API somewhere other than its web host, or shares a bundle id, scheme or host with
another community.

## 4. Credentials

```sh
npx eas credentials --profile riverside
```

Let EAS generate and keep them: the iOS distribution certificate and provisioning profile (it
signs in to the community's Apple account), the Android upload keystore, and the **APNs key**
that push needs on iOS (Android push uses EAS's FCM setup; follow the prompt to upload the
community's FCM v1 service account key). Back up the Android keystore it shows you.

## 5. Build and try it

```sh
npx eas build --profile riverside-preview --platform all   # internal: install from the link
npx eas build --profile riverside --platform all           # store builds
```

Store builds increment the build number on EAS (`appVersionSource: remote`). The app version
(`version` in `app.config.ts`) is the native runtime: JS updates reach builds of the same version.

## 6. Sign in with Apple and Google (server side)

- **Apple:** the iOS build always has the Sign in with Apple entitlement. Add the bundle id to
  the server's accepted audiences: `COURTPIT_APPLE_CLIENT_IDS=app.courtpit.riverside,…`.
- **Google (optional):** in the community's Google Cloud project create three OAuth clients —
  *Web* (its id is the token audience), *iOS* (bundle id; its reversed id is
  `COURTPIT_GOOGLE_IOS_URL_SCHEME`) and *Android* (package name and the SHA-1 from
  `eas credentials`). Put the web and iOS ids in the profile (step 3) and the **web** client id in
  `COURTPIT_GOOGLE_CLIENT_IDS` on the server.

Both server settings are comma-separated lists shared by every community (`docs/deploy.md`).

## 7. Links open the app

With the Apple Team ID and the Android signing certificate's SHA-256 (from
`npx eas credentials`, Android → keystore):

```sh
fly ssh console -C "courtpit-server set-app-links --slug riverside \
  --ios-app-id TEAMID1234.app.courtpit.riverside \
  --android-package app.courtpit.riverside --android-sha256 AB:CD:…:EF"
```

Check `https://riverside.courtpit.app/.well-known/apple-app-site-association` and
`/.well-known/assetlinks.json`. When Play App Signing re-signs the app, add Google's app-signing
certificate SHA-256 as a second `--android-sha256`.

## 8. Submit

```sh
npx eas submit --profile riverside --platform ios       # App Store Connect (asks for the app)
npx eas submit --profile riverside --platform android   # Play: needs a submit profile
```

Export compliance: `app.config.ts` sets `ITSAppUsesNonExemptEncryption` to `false` in the iOS
`Info.plist`, since the app's only encryption is HTTPS and the OS's own (exempt). App Store
Connect then skips the encryption question for every build. If the app ever adds encryption
of its own beyond that, change the key and answer the question again.

For Android, save the Play service account key to `apps/mobile/secrets/` (git-ignored) and add a
`submit` profile like `demo`'s. Start on the `internal` track; promote in the Play Console.

Review notes: give Apple a demo account (a member of the community with a password set) and say
that the app is a club's own app on a shared platform.

## 9. Fix JavaScript without a store release

```sh
npx eas update --channel riverside --message "Fix the score form"
```

Builds of the same app version on that channel pick the update up on their next launch. Native
changes (a new library, a permission, an icon) need a new version and a store build.
