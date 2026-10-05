import { router, useLocalSearchParams } from "expo-router";
import { View } from "react-native";

import { useApi } from "@/api/client";
import { ModerationButton } from "@/features/admin/ModerationButton";
import { moderation } from "@/features/admin/roles";
import { formatUtr, playPrefLabel } from "@/features/format";
import { Detail, DetailsCard, gearLine, socialsLine } from "@/features/players/ProfileDetails";
import { useSignedIn } from "@/session/SessionProvider";
import { space } from "@/theme/tokens";
import { Avatar } from "@/ui/Avatar";
import { Badge } from "@/ui/Badge";
import { Button } from "@/ui/Button";
import { Screen } from "@/ui/Screen";
import { ErrorState, LoadingState } from "@/ui/States";
import { Text } from "@/ui/Text";

/** Another member's profile as the directory shows it (spec §18: contacts only if shared). */
export default function Player() {
  const { id } = useLocalSearchParams<{ id: string }>();
  const { $api } = useApi();
  const session = useSignedIn();
  const me = session.player_id;
  const player = $api.useQuery("get", "/api/v1/players/{id}", { params: { path: { id } } });

  if (player.isPending) return <LoadingState />;
  if (player.error)
    return <ErrorState error={player.error} onRetry={() => void player.refetch()} />;
  const data = player.data;
  const gear = gearLine(data.racket, data.strings, data.tension_kg);
  const socials = socialsLine(data.socials);
  const moderate = moderation({ id: me, role: session.role }, data);

  return (
    <Screen refreshing={player.isRefetching} onRefresh={() => void player.refetch()}>
      <View style={{ alignItems: "center", gap: space.sm }}>
        <Avatar id={data.id} name={data.display_name} size={80} />
        <Text variant="title" align="center" accessibilityRole="header">
          {data.display_name}
        </Text>
        <View style={{ flexDirection: "row", gap: space.sm }}>
          <Badge label={`UTR ${formatUtr(data.utr)}`} tone="primary" />
          {data.role !== "player" ? <Badge label="Club admin" tone="accent" /> : null}
          {data.status === "banned" ? <Badge label="Banned" tone="danger" /> : null}
        </View>
        {data.id !== me && data.status === "active" ? (
          <Button
            label="Challenge to a match"
            icon="tennisball-outline"
            onPress={() =>
              router.push({ pathname: "/players/[id]/challenge", params: { id: data.id } })
            }
          />
        ) : null}
        {moderate ? <ModerationButton player={data} action={moderate} /> : null}
      </View>
      <DetailsCard title="Tennis">
        <Detail icon="tennisball-outline" label="Plays" value={playPrefLabel[data.play_pref]} />
        <Detail
          icon="location-outline"
          label="Likes to play at"
          value={data.preferred_locations.join(", ") || null}
        />
        <Detail icon="construct-outline" label="Racket" value={gear.racket} />
        <Detail icon="git-network-outline" label="Strings" value={gear.strings} />
      </DetailsCard>
      {data.phone || socials ? (
        <DetailsCard title="Contact">
          <Detail icon="call-outline" label="Phone" value={data.phone} />
          <Detail icon="at-outline" label="Socials" value={socials} />
        </DetailsCard>
      ) : null}
    </Screen>
  );
}
