// WinterJS.oauth：授权 URL 拼接与 PKCE 对（全离线纯拼串）。
// WinterJS.oauth: authorization URL building and PKCE pairs (offline pure string ops).
// Run / 运行: winterjs --run sample/wsys/oauth.js
const a = WinterJS.oauth.authorizeUrl({ authUrl: 'https://ex.com/auth', clientId: 'c', redirectUri: 'https://app/cb', scope: 'read', state: 's' });
console.log('[oauth] authorizeUrl:', a.url.includes('response_type=code') && a.state === 's');
const p = WinterJS.oauth.pkce();
console.log('[oauth] pkce:', typeof p.challenge === 'string' && typeof p.verifier === 'string');
