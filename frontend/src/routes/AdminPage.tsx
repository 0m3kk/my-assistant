import { useNavigate, useParams } from "react-router-dom"
import { useTranslation } from "react-i18next"
import { ArrowLeftIcon } from "lucide-react"

import { useAuth } from "@/lib/auth/AuthContext"
import { cn } from "@/lib/utils"
import { Button } from "@/components/ui/button"
import { UserAvatar } from "@/components/shared/UserAvatar"
import {
  Tooltip,
  TooltipContent,
  TooltipProvider,
  TooltipTrigger,
} from "@/components/ui/tooltip"
import { SecretsSection } from "@/components/admin/SecretsSection"
import { ProvidersSection } from "@/components/admin/ProvidersSection"
import { AgentsSection } from "@/components/admin/AgentsSection"
import { ToolsSection } from "@/components/admin/ToolsSection"

const SECTIONS = [
  { id: "providers", labelKey: "admin.providers" },
  { id: "agents", labelKey: "admin.agents" },
  { id: "tools", labelKey: "admin.tools" },
  { id: "secrets", labelKey: "admin.secrets" },
] as const

export function AdminPage() {
  const { t } = useTranslation()
  const { section } = useParams()
  const navigate = useNavigate()
  const { user } = useAuth()
  const active = section ?? "providers"

  return (
    <div className="mx-auto flex min-h-dvh max-w-5xl flex-col p-6">
      <div className="flex items-center justify-between pb-4">
        <div className="flex items-center gap-2">
          <Button variant="outline" size="icon-sm" onClick={() => navigate("/chat")} aria-label="Back">
            <ArrowLeftIcon />
          </Button>
          <h1 className="text-xl font-semibold">{t("admin.title")}</h1>
        </div>
        <div className="flex items-center gap-3 text-xs text-muted-foreground">
          {user && (
            <TooltipProvider>
              <Tooltip>
                <TooltipTrigger asChild>
                  <UserAvatar user={user} className="cursor-default" />
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
      </div>

      <nav className="flex gap-1 border-b pb-2">
        {SECTIONS.map((s) => (
          <Button
            key={s.id}
            variant={active === s.id ? "secondary" : "ghost"}
            size="sm"
            className={cn(active === s.id && "font-medium")}
            onClick={() => navigate(`/admin/${s.id}`)}
          >
            {t(s.labelKey)}
          </Button>
        ))}
      </nav>

      <div className="pt-6">
        {active === "providers" && <ProvidersSection />}
        {active === "agents" && <AgentsSection />}
        {active === "tools" && <ToolsSection />}
        {active === "secrets" && <SecretsSection />}
      </div>
    </div>
  )
}
