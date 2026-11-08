import { createContext, useCallback, useContext, useEffect, useMemo, useState } from "react"
import type { ReactNode } from "react"

import { getToken, setToken } from "../api/client"
import { authApi } from "../api/endpoints"
import type { PublicUser } from "../types"

interface AuthContextValue {
  user: PublicUser | null
  loading: boolean
  login: (email: string, password: string) => Promise<PublicUser>
  register: (email: string, password: string, name: string) => Promise<PublicUser>
  updateProfile: (body: { name?: string; avatar_url?: string | null }) => Promise<PublicUser>
  logout: () => void
}

const AuthContext = createContext<AuthContextValue | null>(null)

export function AuthProvider({ children }: { children: ReactNode }) {
  const [user, setUser] = useState<PublicUser | null>(null)
  const [loading, setLoading] = useState(!!getToken())

  useEffect(() => {
    if (!getToken()) {
      setUser(null)
      setLoading(false)
      return
    }
    authApi
      .me()
      .then(setUser)
      .catch(() => setToken(null))
      .finally(() => setLoading(false))
  }, [])

  const login = useCallback(async (email: string, password: string) => {
    const res = await authApi.login({ email, password })
    setToken(res.token)
    setUser(res.user)
    return res.user
  }, [])

  const register = useCallback(async (email: string, password: string, name: string) => {
    const res = await authApi.register({ email, password, name })
    setToken(res.token)
    setUser(res.user)
    return res.user
  }, [])

  const updateProfile = useCallback(
    async (body: { name?: string; avatar_url?: string | null }) => {
      const updated = await authApi.updateProfile(body)
      setUser(updated)
      return updated
    },
    [],
  )

  const logout = useCallback(() => {
    setToken(null)
    setUser(null)
  }, [])

  const value = useMemo(
    () => ({ user, loading, login, register, updateProfile, logout }),
    [user, loading, login, register, updateProfile, logout],
  )

  return <AuthContext.Provider value={value}>{children}</AuthContext.Provider>
}

export function useAuth(): AuthContextValue {
  const ctx = useContext(AuthContext)
  if (!ctx) throw new Error("useAuth must be used within AuthProvider")
  return ctx
}
