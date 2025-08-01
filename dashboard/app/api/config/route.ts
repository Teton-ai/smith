export async function GET() {
  return Response.json({
    env: {
      API_BASE_URL: process.env.API_BASE_URL,
      AUTH0_DOMAIN: process.env.AUTH0_DOMAIN,
      AUTH0_CLIENT_ID: process.env.AUTH0_CLIENT_ID,
      AUTH0_REDIRECT_URI: process.env.AUTH0_REDIRECT_URI,
      AUTH0_AUDIENCE: process.env.AUTH0_AUDIENCE
    }
  });
}
