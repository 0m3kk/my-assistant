import { useState } from "react"
import { useMutation } from "@tanstack/react-query"
import { useNavigate } from "react-router-dom"
import { useTranslation } from "react-i18next"
import { ArrowLeftIcon } from "lucide-react"
import { toast } from "sonner"

import { useAuth } from "@/lib/auth/AuthContext"
import { ApiError } from "@/lib/api/client"
import { UserAvatar } from "@/components/shared/UserAvatar"
import { Button } from "@/components/ui/button"
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card"
import { Input } from "@/components/ui/input"
import { Label } from "@/components/ui/label"
import { Separator } from "@/components/ui/separator"

export function ProfilePage() {
  const { t } = useTranslation()
  const navigate = useNavigate()
  const { user, updateProfile } = useAuth()
  const [name, setName] = useState(user?.name ?? "")
  const [avatarUrl, setAvatarUrl] = useState(user?.avatar_url ?? "")

  const save = useMutation({
    mutationFn: () => updateProfile({ name: name.trim(), avatar_url: avatarUrl.trim() || null }),
    onSuccess: () => toast.success(t("profile.saved")),
    onError: (e) => toast.error(e instanceof ApiError ? e.message : String(e)),
  })

  if (!user) return null

  // Live preview while editing the avatar URL / display name.
  const preview = { ...user, name, avatar_url: avatarUrl.trim() || null }

  return (
    <div className="mx-auto flex min-h-dvh max-w-2xl flex-col p-6">
      <div className="flex items-center justify-between pb-6">
        <div className="flex items-center gap-2">
          <Button variant="outline" size="icon-sm" onClick={() => navigate("/chat")} aria-label="Back">
            <ArrowLeftIcon />
          </Button>
          <h1 className="text-xl font-semibold">{t("profile.title")}</h1>
        </div>
        <UserAvatar user={preview} size="default" />
      </div>

      <Card>
        <CardHeader>
          <CardTitle>{t("profile.title")}</CardTitle>
        </CardHeader>
        <CardContent className="grid gap-6">
          <div className="flex items-center gap-4">
            <UserAvatar user={preview} size="lg" />
            <div className="min-w-0 text-sm">
              <p className="truncate">{user.email}</p>
              <p className="capitalize text-muted-foreground">{user.role}</p>
            </div>
          </div>

          <Separator />

          <div className="grid gap-2">
            <Label htmlFor="profile-name">{t("profile.displayName")}</Label>
            <Input
              id="profile-name"
              value={name}
              onChange={(e) => setName(e.target.value)}
              placeholder={user.email}
            />
          </div>

          <div className="grid gap-2">
            <Label htmlFor="profile-avatar">{t("profile.avatarUrl")}</Label>
            <Input
              id="profile-avatar"
              value={avatarUrl}
              onChange={(e) => setAvatarUrl(e.target.value)}
              placeholder="https://…"
            />
            <p className="text-xs text-muted-foreground">{t("profile.avatarUrlHint")}</p>
          </div>

          <div className="flex justify-end">
            <Button onClick={() => save.mutate()} disabled={save.isPending}>
              {t("profile.save")}
            </Button>
          </div>
        </CardContent>
      </Card>
    </div>
  )
}
