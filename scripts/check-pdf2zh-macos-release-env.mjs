import { execFileSync } from 'node:child_process'

const requireMacosRelease = process.env.LUMENFOLIO_PDF2ZH_REQUIRE_MACOS_RELEASE === '1'
const identity = process.env.LUMENFOLIO_PDF2ZH_CODESIGN_IDENTITY || ''
const notaryProfile = process.env.LUMENFOLIO_PDF2ZH_NOTARYTOOL_PROFILE || ''
const appleId = process.env.APPLE_ID || ''
const appleTeamId = process.env.APPLE_TEAM_ID || ''
const applePassword = process.env.APPLE_APP_SPECIFIC_PASSWORD || ''
const probeNotary = process.env.LUMENFOLIO_PDF2ZH_PROBE_NOTARY === '1'

function fail(message) {
  console.error(message)
  process.exit(1)
}

function assert(condition, message) {
  if (!condition) fail(message)
}

function capture(command, args) {
  return execFileSync(command, args, { encoding: 'utf8', stdio: 'pipe' })
}

function codesignIdentities() {
  try {
    return capture('security', ['find-identity', '-v', '-p', 'codesigning'])
  } catch (error) {
    fail(`Unable to read macOS codesigning identities: ${error.message}`)
  }
}

function probeNotaryCredentials() {
  if (!probeNotary) return false
  const args = ['notarytool', 'history']
  if (notaryProfile) {
    args.push('--keychain-profile', notaryProfile)
  } else {
    args.push('--apple-id', appleId, '--team-id', appleTeamId, '--password', applePassword)
  }
  capture('xcrun', args)
  return true
}

if (process.platform !== 'darwin') {
  console.log(JSON.stringify({
    ok: !requireMacosRelease,
    skipped: true,
    reason: 'macOS release signing verification only runs on darwin',
  }, null, 2))
  if (requireMacosRelease) process.exit(1)
  process.exit(0)
}

if (!requireMacosRelease && (!identity || identity === '-')) {
  console.log(JSON.stringify({
    ok: true,
    skipped: true,
    reason: 'Set LUMENFOLIO_PDF2ZH_REQUIRE_MACOS_RELEASE=1 to require Developer ID and notarization credentials',
  }, null, 2))
  process.exit(0)
}

assert(identity && identity !== '-', 'LUMENFOLIO_PDF2ZH_CODESIGN_IDENTITY must be a Developer ID Application identity')
assert(identity.includes('Developer ID Application'), 'Codesign identity must be a Developer ID Application identity')

const identities = codesignIdentities()
assert(
  identities.includes(identity),
  `Developer ID identity not found in keychain: ${identity}`,
)

const hasProfileCredentials = Boolean(notaryProfile)
const hasAppleIdCredentials = Boolean(appleId && appleTeamId && applePassword)
assert(
  hasProfileCredentials || hasAppleIdCredentials,
  'Missing notarization credentials: set LUMENFOLIO_PDF2ZH_NOTARYTOOL_PROFILE or APPLE_ID/APPLE_TEAM_ID/APPLE_APP_SPECIFIC_PASSWORD',
)

const notaryProbed = probeNotaryCredentials()

console.log(JSON.stringify({
  ok: true,
  identity,
  identityFound: true,
  notarizationCredentials: hasProfileCredentials ? 'keychain-profile' : 'apple-id',
  notaryProbed,
}, null, 2))
