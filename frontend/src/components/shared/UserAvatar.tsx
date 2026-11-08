import type { PublicUser } from "@/lib/types"
import { Avatar, AvatarFallback, AvatarImage } from "@/components/ui/avatar"

/** User avatar: image when `avatar_url` is set, otherwise the name initials. */
export function UserAvatar({
  user,
  size = "sm",
  className,
  onClick,
}: {
  user: Pick<PublicUser, "email" | "name" | "avatar_url">
  size?: "default" | "sm" | "lg"
  className?: string
  onClick?: () => void
}) {
  const label = user.name || user.email
  return (
    <Avatar size={size} className={className} onClick={onClick}>
      {user.avatar_url && <AvatarImage src={user.avatar_url} alt={label} />}
      <AvatarFallback className="bg-primary font-medium text-primary-foreground">
        {label.charAt(0).toUpperCase()}
      </AvatarFallback>
    </Avatar>
  )
}
