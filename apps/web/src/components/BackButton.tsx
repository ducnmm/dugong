import React from 'react';
import { useNavigate } from 'react-router-dom';
import { ArrowLeft } from 'lucide-react';

interface BackButtonProps {
  /** Where to go when there is no in-app history to return to (direct visit, shared link). */
  fallback?: string;
  /** Button label. */
  label?: string;
  className?: string;
}

/**
 * Neo-brutalist back button.
 *
 * Prefers stepping back through the SPA history (`navigate(-1)`) so the user
 * returns to wherever they came from. React Router tracks the current entry's
 * position in `window.history.state.idx`; when it is 0 the user landed here
 * directly (e.g. a shared link) and going back would leave the app, so we
 * navigate to `fallback` instead.
 */
export const BackButton: React.FC<BackButtonProps> = ({
  fallback = '/',
  label = 'Back',
  className = '',
}) => {
  const navigate = useNavigate();

  const handleBack = () => {
    const idx = (window.history.state as { idx?: number } | null)?.idx ?? 0;
    if (idx > 0) {
      navigate(-1);
    } else {
      navigate(fallback);
    }
  };

  return (
    <button
      type="button"
      onClick={handleBack}
      aria-label={label}
      className={`inline-flex items-center gap-2 rounded-full border-2 border-black bg-white px-4 py-2 text-sm font-black text-black shadow-neo-sm transition-colors hover:bg-yellow-200 ${className}`}
    >
      <ArrowLeft className="h-4 w-4" />
      {label}
    </button>
  );
};
