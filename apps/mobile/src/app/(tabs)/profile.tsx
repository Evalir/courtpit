import { router } from "expo-router";
import { View } from "react-native";

import { useApi } from "@/api/client";
import { formatDate, formatUtr, playPrefLabel } from "@/features/format";
import { Detail, DetailsCard, gearLine, socialsLine } from "@/features/players/ProfileDetails";
import { isAdmin } from "@/features/admin/roles";
import { ExportButton } from "@/features/profile/ExportButton";
import { useSession } from "@/session/SessionProvider";
import { useCommunity } from "@/tenant/TenantProvider";
import { space } from "@/theme/tokens";
import { Avatar } from "@/ui/Avatar";
import { Badge } from "@/ui/Badge";
import { Button } from "@/ui/Button";
import { Screen } from "@/ui/Screen";
import { ErrorState, LoadingState } from "@/ui/States";
import { Text } from "@/ui/Text";

const providerLabel: Record<string, string> = { apple: "Apple", google: "Google" };

/** Profile: the signed-in player's own view of their profile and account. */
export default function Profile() {
  const { $api } = useApi();
  const { signOut } = useSession();
  const community = useCommunity();
  const me = $api.useQuery("get", "/api/v1/me");

  if (me.isPending) return <LoadingState />;
  if (me.error) return <ErrorState error={me.error} onRetry={() => void me.refetch()} />;
  const { player, account } = me.data;
  const gear = gearLine(player.racket, player.strings, player.tension_kg);
  const visibility = (shown: boolean) => (shown ? "Shown to verified members" : "Only you");

  return (
    <Screen title="Profile" refreshing={me.isRefetching} onRefresh={() => void me.refetch()}>
      <View style={{ alignItems: "center", gap: space.sm }}>
        <Avatar id={player.id} name={player.display_name} size={80} />
        <Text variant="title" align="center" accessibilityRole="header">
          {player.display_name}
        </Text>
        <View style={{ flexDirection: "row", gap: space.sm }}>
          <Badge label={`UTR ${formatUtr(player.utr)}`} tone="primary" />
          {player.role !== "player" ? (
            <Badge label={player.role === "owner" ? "Owner" : "Admin"} tone="accent" />
          ) : null}
        </View>
        <Text variant="caption" tone="textMuted">
          Member of {community.name} since {formatDate(player.created_at)}
        </Text>
        <View style={{ flexDirection: "row", gap: space.sm, marginTop: space.sm }}>
          <Button
            label="Edit profile"
            variant="secondary"
            size="sm"
            icon="create-outline"
            onPress={() => router.push("/profile/edit")}
          />
          <Button
            label="Points history"
            variant="secondary"
            size="sm"
            icon="podium-outline"
            onPress={() => router.push("/rankings/me")}
          />
        </View>
      </View>
      <DetailsCard title="Tennis">
        <Detail icon="tennisball-outline" label="Plays" value={playPrefLabel[player.play_pref]} />
        <Detail
          icon="location-outline"
          label="Likes to play at"
          value={player.preferred_locations.join(", ") || "Anywhere"}
        />
        <Detail icon="construct-outline" label="Racket" value={gear.racket} />
        <Detail icon="git-network-outline" label="Strings" value={gear.strings} />
      </DetailsCard>
      <DetailsCard title="Contact">
        <Detail
          icon="call-outline"
          label={`Phone · ${visibility(player.phone_visible)}`}
          value={player.phone ?? "Not set"}
        />
        <Detail
          icon="at-outline"
          label={`Socials · ${visibility(player.socials_visible)}`}
          value={socialsLine(player.socials) ?? "Not set"}
        />
      </DetailsCard>
      <DetailsCard title="Account">
        <Detail
          icon="mail-outline"
          label={account.email_verified ? "Email · verified" : "Email · not verified"}
          value={account.email}
        />
        <Detail
          icon="key-outline"
          label="Sign-in"
          value={[
            "Email code",
            account.has_password ? "Password" : null,
            ...account.identities.map((provider) => providerLabel[provider] ?? provider),
          ]
            .filter(Boolean)
            .join(", ")}
        />
      </DetailsCard>
      {isAdmin(player.role) ? (
        <Button
          label="Club admin"
          variant="secondary"
          icon="shield-checkmark-outline"
          block
          onPress={() => router.push("/admin")}
        />
      ) : null}
      <Button
        label={account.has_password ? "Change password" : "Set a password"}
        variant="secondary"
        icon="key-outline"
        block
        onPress={() => router.push("/profile/password")}
      />
      <ExportButton />
      <Button
        label="Sign out"
        variant="danger"
        icon="log-out-outline"
        block
        onPress={() => void signOut()}
      />
      <Button
        label="Delete account"
        variant="ghost"
        block
        onPress={() => router.push("/profile/delete")}
      />
    </Screen>
  );
}
