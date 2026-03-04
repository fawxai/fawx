export const config = { runtime: 'edge' };

/**
 * Key delivery endpoint. Returns server-side API keys that the app needs
 * for direct connections (e.g., TinyFish SSE streaming).
 *
 * Requires Bearer auth with the compiled app token. This prevents
 * unauthenticated access — only APK builds with the matching token
 * can fetch keys. Token is rotated via scripts/release.sh.
 *
 * The app token itself is NOT returned here (it's compiled into the APK).
 * Only third-party keys that need server-side rotation are delivered.
 */
export default async function handler(req: Request) {
  if (req.method === 'OPTIONS') {
    return new Response(null, { status: 204, headers: corsHeaders() });
  }

  if (req.method !== 'POST') {
    return jsonError('Method not allowed', 405);
  }

  const env = (globalThis as { process?: { env?: Record<string, string | undefined> } })
    .process?.env ?? {};

  // Require Bearer auth with the compiled app token.
  const appToken = env.FAWX_APP_TOKEN;
  if (!appToken) {
    return jsonError('Service not configured', 503);
  }

  const auth = req.headers.get('Authorization');
  if (auth !== `Bearer ${appToken}`) {
    return jsonError('Unauthorized', 401);
  }

  // Only deliver third-party keys — app token is compiled into APK.
  const keys: Record<string, string | undefined> = {};

  if (env.TINYFISH_API_KEY) {
    keys.tinyfish = env.TINYFISH_API_KEY;
  }

  // Add future keys here as needed:
  // if (env.SOME_OTHER_KEY) keys.someOther = env.SOME_OTHER_KEY;

  return new Response(JSON.stringify({ keys }), {
    status: 200,
    headers: { 'Content-Type': 'application/json', ...corsHeaders() },
  });
}

function jsonError(message: string, status: number): Response {
  return new Response(JSON.stringify({ error: message }), {
    status,
    headers: { 'Content-Type': 'application/json', ...corsHeaders() },
  });
}

function corsHeaders(): Record<string, string> {
  return {
    'Access-Control-Allow-Origin': '*',
    'Access-Control-Allow-Methods': 'POST, OPTIONS',
    'Access-Control-Allow-Headers': 'Content-Type, Authorization',
  };
}
