import { useEffect, useState, useRef } from 'react';
import { useNavigate, useSearchParams } from 'react-router-dom';
import { useXAuth } from '../hooks/useXAuth';
import { useAuth } from '../contexts/useAuth';
import { CheckCircle, XCircle, Loader2 } from 'lucide-react';

export const Callback: React.FC = () => {
  const [searchParams] = useSearchParams();
  const navigate = useNavigate();
  const { handleCallback, error: authError } = useXAuth();
  const { login } = useAuth();
  const [error, setError] = useState<string | null>(null);
  const [status, setStatus] = useState<'processing' | 'success' | 'error'>('processing');

  // Prevent double execution in StrictMode
  const hasProcessed = useRef(false);

  useEffect(() => {
    const processCallback = async () => {
      // Skip if already processed (StrictMode double-render protection)
      if (hasProcessed.current) return;
      hasProcessed.current = true;
      // Get OAuth parameters from URL
      const code = searchParams.get('code');
      const state = searchParams.get('state');
      const errorParam = searchParams.get('error');
      const errorDescription = searchParams.get('error_description');

      // Handle X OAuth errors
      if (errorParam) {
        setError(errorDescription || `OAuth error: ${errorParam}`);
        setStatus('error');
        return;
      }

      // Validate required parameters
      if (!code || !state) {
        setError('Missing authorization code or state parameter');
        setStatus('error');
        return;
      }

      try {
        // Exchange code for token and get user info
        const result = await handleCallback(code, state);

        // Update auth context with user info and access token
        login(
          {
            twitterUserId: result.user.id,
            twitterHandle: result.user.username,
            suiObjectId: result.dugongAccount?.sui_object_id || null,
            linkedWalletAddress: result.dugongAccount?.owner_address || null,
          },
          result.accessToken
        );

        setStatus('success');

        // Redirect to dashboard after short delay
        setTimeout(() => {
          navigate('/dashboard', { replace: true });
        }, 1500);
      } catch (err) {
        setError(err instanceof Error ? err.message : 'Authentication failed');
        setStatus('error');
      }
    };

    processCallback();
  }, [searchParams, handleCallback, login, navigate]);

  return (
    <div className="neo-page flex min-h-screen items-center justify-center p-4">
      <div className="glass-strong w-full max-w-md p-8 text-center">
        {status === 'processing' && (
          <>
            <div className="neo-icon-tile mx-auto mb-6 h-16 w-16 bg-cyan-200">
              <Loader2 className="h-8 w-8 animate-spin text-black" />
            </div>
            <h2 className="hero-font mb-2 text-4xl font-black text-black">
              Completing Sign In...
            </h2>
            <p className="font-bold text-gray-700">
              Please wait while we verify your X account.
            </p>
          </>
        )}

        {status === 'success' && (
          <>
            <div className="neo-icon-tile mx-auto mb-6 h-16 w-16 bg-lime-200">
              <CheckCircle className="h-8 w-8 text-black" />
            </div>
            <h2 className="hero-font mb-2 text-4xl font-black text-black">
              Successfully Signed In!
            </h2>
            <p className="font-bold text-gray-700">
              Redirecting to dashboard...
            </p>
            <div className="mt-4 flex justify-center">
              <div className="flex space-x-1">
                {[0, 1, 2].map((i) => (
                  <div
                    key={i}
                    className="h-2 w-2 animate-pulse rounded-full border-2 border-black bg-yellow-200"
                    style={{ animationDelay: `${i * 0.2}s` }}
                  />
                ))}
              </div>
            </div>
          </>
        )}

        {status === 'error' && (
          <>
            <div className="neo-icon-tile mx-auto mb-6 h-16 w-16 bg-red-200">
              <XCircle className="h-8 w-8 text-black" />
            </div>
            <h2 className="hero-font mb-2 text-4xl font-black text-black">
              Sign In Failed
            </h2>
            <p className="mb-6 font-bold text-gray-700">
              {error || authError || 'An unknown error occurred'}
            </p>
            <button
              onClick={() => navigate('/', { replace: true })}
              className="btn-sui w-full"
            >
              Try Again
            </button>
          </>
        )}
      </div>
    </div>
  );
};
