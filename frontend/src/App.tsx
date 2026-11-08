import { BrowserRouter, Navigate, Route, Routes } from "react-router-dom"

import { useAuth } from "./lib/auth/AuthContext"
import { AdminPage } from "./routes/AdminPage"
import { ChatPage } from "./routes/ChatPage"
import { LoginPage } from "./routes/LoginPage"
import { ProfilePage } from "./routes/ProfilePage"
import { RegisterPage } from "./routes/RegisterPage"
import { SettingsPage } from "./routes/SettingsPage"
import { StudioPage } from "./routes/StudioPage"
import { Spinner } from "./components/shared/Spinner"

function RequireAuth({ children }: { children: React.ReactNode }) {
  const { user, loading } = useAuth()
  if (loading) {
    return (
      <div className="flex h-dvh items-center justify-center">
        <Spinner />
      </div>
    )
  }
  if (!user) return <Navigate to="/login" replace />
  return <>{children}</>
}

function RequireAdmin({ children }: { children: React.ReactNode }) {
  const { user } = useAuth()
  if (user?.role !== "admin") return <Navigate to="/chat" replace />
  return <>{children}</>
}

export default function App() {
  return (
    <BrowserRouter>
      <Routes>
        <Route path="/login" element={<LoginPage />} />
        <Route path="/register" element={<RegisterPage />} />
        <Route
          path="/chat/:conversationId?"
          element={
            <RequireAuth>
              <ChatPage />
            </RequireAuth>
          }
        />
        <Route
          path="/studio/:meta?"
          element={
            <RequireAuth>
              <RequireAdmin>
                <StudioPage />
              </RequireAdmin>
            </RequireAuth>
          }
        />
        <Route
          path="/settings"
          element={
            <RequireAuth>
              <SettingsPage />
            </RequireAuth>
          }
        />
        <Route
          path="/profile"
          element={
            <RequireAuth>
              <ProfilePage />
            </RequireAuth>
          }
        />
        <Route
          path="/admin/:section?"
          element={
            <RequireAuth>
              <RequireAdmin>
                <AdminPage />
              </RequireAdmin>
            </RequireAuth>
          }
        />
        <Route path="*" element={<Navigate to="/chat" replace />} />
      </Routes>
    </BrowserRouter>
  )
}
