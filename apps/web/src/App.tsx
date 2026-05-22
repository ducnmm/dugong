import { BrowserRouter as Router, Routes, Route, Navigate } from 'react-router-dom';
import { AuthProvider } from './contexts/AuthContext';
import { useAuth } from './contexts/useAuth';
import { CustomWalletProvider } from './contexts/CustomWalletContext';
import { Home } from './pages/Home';
import { Dashboard } from './pages/Dashboard';
import { AccountView } from './pages/AccountView';
import { Callback } from './pages/Callback';

function AppRoutes() {
  const { isAuthenticated, isLoading } = useAuth();

  // Wait for auth state to be loaded from localStorage before rendering routes
  if (isLoading) {
    return (
      <div className="neo-page flex items-center justify-center">
        <div className="h-16 w-16 animate-spin rounded-full border-4 border-black border-t-cyan-300 bg-white shadow-neo-md" />
      </div>
    );
  }

  return (
    <Router>
      <Routes>
        {/* Public Routes - Home page accessible to everyone */}
        <Route path="/" element={<Home />} />

        {/* OAuth Callback - handles X login redirect */}
        <Route path="/callback" element={<Callback />} />

        {/* Public route - View any account by twitter_id */}
        <Route path="/account/:twitter_id" element={<AccountView />} />

        {/* Protected Routes - User's own dashboard */}
        <Route
          path="/dashboard"
          element={
            isAuthenticated ? <Navigate to="/dashboard/activity" replace /> : <Navigate to="/" replace />
          }
        />
        <Route
          path="/dashboard/:tab"
          element={
            isAuthenticated ? <Dashboard /> : <Navigate to="/" replace />
          }
        />

        {/* Catch all - redirect to home */}
        <Route path="*" element={<Navigate to="/" replace />} />
      </Routes>
    </Router>
  );
}

function App() {
  return (
    <AuthProvider>
      <CustomWalletProvider>
        <AppRoutes />
      </CustomWalletProvider>
    </AuthProvider>
  );
}

export default App;
