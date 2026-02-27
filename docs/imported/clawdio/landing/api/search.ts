export const config = { runtime: 'edge' };

const BRAVE_ENDPOINT = 'https://api.search.brave.com/res/v1/web/search';
const MAX_COUNT = 5;
const DEFAULT_COUNT = 3;

interface SearchRequest {
  query: string;
  count?: number;
}

export default async function handler(req: Request) {
  // CORS preflight
  if (req.method === 'OPTIONS') {
    return new Response(null, {
      status: 204,
      headers: corsHeaders(),
    });
  }

  if (req.method !== 'POST') {
    return jsonError('Method not allowed', 405);
  }

  const env = (globalThis as { process?: { env?: Record<string, string | undefined> } })
    .process?.env ?? {};

  // App token auth — same as /api/keys
  const appToken = env.FAWX_APP_TOKEN;
  if (appToken) {
    const auth = req.headers.get('Authorization');
    if (auth !== `Bearer ${appToken}`) {
      return jsonError('Unauthorized', 401);
    }
  }

  const apiKey = env.BRAVE_API_KEY;
  if (!apiKey) {
    return jsonError('Search service not configured', 503);
  }

  let body: SearchRequest;
  try {
    body = await req.json();
  } catch (_) {
    return jsonError('Invalid JSON body', 400);
  }

  const { query, count } = body;
  if (!query || typeof query !== 'string' || query.trim().length === 0) {
    return jsonError('Missing or empty query', 400);
  }

  const clampedCount = Math.min(Math.max(count ?? DEFAULT_COUNT, 1), MAX_COUNT);

  const url = new URL(BRAVE_ENDPOINT);
  url.searchParams.set('q', query.trim());
  url.searchParams.set('count', String(clampedCount));

  try {
    const res = await fetch(url.toString(), {
      headers: {
        'Accept': 'application/json',
        'X-Subscription-Token': apiKey,
      },
    });

    if (!res.ok) {
      await res.text().catch(() => '');
      return jsonError(`Brave API error (${res.status})`, res.status >= 500 ? 502 : res.status);
    }

    const data = await res.json();
    return new Response(JSON.stringify(data), {
      status: 200,
      headers: {
        'Content-Type': 'application/json',
        ...corsHeaders(),
      },
    });
  } catch (_) {
    return jsonError('Search request failed', 502);
  }
}

function jsonError(message: string, status: number): Response {
  return new Response(JSON.stringify({ error: message }), {
    status,
    headers: {
      'Content-Type': 'application/json',
      ...corsHeaders(),
    },
  });
}

function corsHeaders(): Record<string, string> {
  return {
    'Access-Control-Allow-Origin': '*',
    'Access-Control-Allow-Methods': 'POST, OPTIONS',
    'Access-Control-Allow-Headers': 'Content-Type, Authorization',
  };
}
