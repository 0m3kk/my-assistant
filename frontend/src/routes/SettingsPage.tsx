import { useQuery, useQueryClient } from "@tanstack/react-query"
import { useNavigate } from "react-router-dom"
import { useTranslation } from "react-i18next"
import { toast } from "sonner"
import { ArrowLeftIcon } from "lucide-react"

import { modelsApi, preferencesApi } from "@/lib/api/endpoints"
import { useAuth } from "@/lib/auth/AuthContext"
import { setLocale } from "@/lib/i18n"
import { usePreferences, type FontScale, type Theme } from "@/lib/theme/PreferencesProvider"
import { Button } from "@/components/ui/button"
import { UserAvatar } from "@/components/shared/UserAvatar"
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card"
import { Label } from "@/components/ui/label"
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select"
import { Separator } from "@/components/ui/separator"
import {
  Tooltip,
  TooltipContent,
  TooltipProvider,
  TooltipTrigger,
} from "@/components/ui/tooltip"

export function SettingsPage() {
  const { t } = useTranslation()
  const { user } = useAuth()
  const navigate = useNavigate()
  const { theme, fontScale, setTheme, setFontScale } = usePreferences()
  const queryClient = useQueryClient()

  const { data: prefs } = useQuery({ queryKey: ["preferences"], queryFn: () => preferencesApi.get() })
  const { data: models } = useQuery({ queryKey: ["models"], queryFn: () => modelsApi.list() })

  async function updateLocale(locale: string) {
    try {
      await preferencesApi.update({ locale })
      setLocale(locale)
      void queryClient.invalidateQueries({ queryKey: ["preferences"] })
      toast.success(t("settings.saved"))
    } catch (e) {
      toast.error(String(e))
    }
  }

  async function updateTitleModel(value: string) {
    try {
      await preferencesApi.update({ title_model_id: value === "default" ? null : value })
      void queryClient.invalidateQueries({ queryKey: ["preferences"] })
      toast.success(t("settings.saved"))
    } catch (e) {
      toast.error(String(e))
    }
  }

  return (
    <div className="mx-auto flex min-h-dvh max-w-2xl flex-col p-6">
      <div className="flex items-center justify-between pb-6">
        <div className="flex items-center gap-2">
          <Button variant="outline" size="icon-sm" onClick={() => navigate("/chat")} aria-label="Back">
            <ArrowLeftIcon />
          </Button>
          <h1 className="text-xl font-semibold">{t("settings.title")}</h1>
        </div>
        {user && (
          <TooltipProvider>
            <Tooltip>
              <TooltipTrigger asChild>
                <UserAvatar user={user} className="cursor-pointer" onClick={() => navigate("/profile")} />
              </TooltipTrigger>
              <TooltipContent side="bottom" align="end">
                <div className="grid gap-0.5">
                  {user.name && <span className="font-medium">{user.name}</span>}
                  <span className="opacity-80">{user.email}</span>
                  <span className="capitalize opacity-80">{user.role}</span>
                </div>
              </TooltipContent>
            </Tooltip>
          </TooltipProvider>
        )}
      </div>

      <Card>
        <CardHeader>
          <CardTitle>{t("settings.title")}</CardTitle>
        </CardHeader>
        <CardContent className="grid gap-6">
          <div className="grid gap-2">
            <Label htmlFor="theme">{t("settings.theme")}</Label>
            <Select
              value={theme}
              onValueChange={(v) => setTheme(v as Theme)}
            >
              <SelectTrigger id="theme" className="w-48">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="system">{t("settings.themeSystem")}</SelectItem>
                <SelectItem value="light">{t("settings.themeLight")}</SelectItem>
                <SelectItem value="dark">{t("settings.themeDark")}</SelectItem>
              </SelectContent>
            </Select>
          </div>

          <Separator />

          <div className="grid gap-2">
            <Label htmlFor="font">{t("settings.fontSize")}</Label>
            <Select
              value={fontScale}
              onValueChange={(v) => setFontScale(v as FontScale)}
            >
              <SelectTrigger id="font" className="w-48">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="sm">{t("settings.fontSmall")}</SelectItem>
                <SelectItem value="md">{t("settings.fontMedium")}</SelectItem>
                <SelectItem value="lg">{t("settings.fontLarge")}</SelectItem>
                <SelectItem value="xl">{t("settings.fontXLarge")}</SelectItem>
              </SelectContent>
            </Select>
          </div>

          <Separator />

          <div className="grid gap-2">
            <Label htmlFor="locale">{t("settings.language")}</Label>
            <Select value={prefs?.locale ?? "en"} onValueChange={(v) => void updateLocale(v)}>
              <SelectTrigger id="locale" className="w-48">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="en">English</SelectItem>
              </SelectContent>
            </Select>
            <p className="text-xs text-muted-foreground">More languages coming — add a locale file to enable one.</p>
          </div>

          <Separator />

          <div className="grid gap-2">
            <Label htmlFor="title-model">{t("settings.titleModel")}</Label>
            <Select
              value={prefs?.title_model_id ?? "default"}
              onValueChange={(v) => void updateTitleModel(v)}
            >
              <SelectTrigger id="title-model" className="w-64">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="default">{t("settings.titleModelDefault")}</SelectItem>
                {models?.map((m) => (
                  <SelectItem key={m.id} value={m.id}>
                    {m.display_name}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
            <p className="text-xs text-muted-foreground">{t("settings.titleModelHint")}</p>
          </div>
        </CardContent>
      </Card>
    </div>
  )
}
