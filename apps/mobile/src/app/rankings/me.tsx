import type { components } from "@racquetcollective/api-client";
import { router } from "expo-router";
import { useState } from "react";
import { View } from "react-native";

import { useApi } from "@/api/client";
import { disciplineLabel, formatDate } from "@/features/format";
import {
  byMonth,
  ledgerTarget,
  pointsLabel,
  sourceLabel,
  stillCounts,
  type LedgerEntry,
} from "@/features/rankings/ledger";
import { useSignedIn } from "@/session/SessionProvider";
import { useCommunity } from "@/tenant/TenantProvider";
import { space } from "@/theme/tokens";
import { Card } from "@/ui/Card";
import { Screen, Section } from "@/ui/Screen";
import { Segmented } from "@/ui/Segmented";
import { EmptyState, ErrorState, LoadingState } from "@/ui/States";
import { Text } from "@/ui/Text";

type Discipline = components["schemas"]["Discipline"];

/** The viewer's points ledger: every result behind their ranking, newest first. */
export default function PointsHistory() {
  const me = useSignedIn().player_id;
  const { features } = useCommunity();
  const { $api } = useApi();
  const options = [
    { value: "all" as const, label: "All" },
    { value: "singles" as const, label: "Singles" },
    ...(features.doubles ? [{ value: "doubles" as const, label: "Doubles" }] : []),
    ...(features.mixed ? [{ value: "mixed" as const, label: "Mixed" }] : []),
  ];
  const [discipline, setDiscipline] = useState<Discipline | "all">("all");
  const ledger = $api.useQuery("get", "/api/v1/rankings/events", {
    params: {
      query: { player_id: me, ...(discipline === "all" ? {} : { discipline }) },
    },
  });
  const now = new Date(ledger.dataUpdatedAt);

  return (
    <Screen refreshing={ledger.isRefetching} onRefresh={() => void ledger.refetch()}>
      <View style={{ gap: space.md }}>
        {options.length > 2 ? (
          <Segmented options={options} value={discipline} onChange={setDiscipline} />
        ) : null}
        <Text variant="caption" tone="textMuted">
          Your ranking adds up the points of the last 52 weeks; older results stay here, greyed out.
          Showing your latest 100.
        </Text>
      </View>
      {ledger.isPending ? (
        <LoadingState />
      ) : ledger.error ? (
        <ErrorState error={ledger.error} onRetry={() => void ledger.refetch()} />
      ) : ledger.data.length === 0 ? (
        <EmptyState
          icon="podium-outline"
          title="No points yet"
          body="League matches and season finishes earn ranking points."
        />
      ) : (
        byMonth(ledger.data).map((group) => (
          <Section key={group.month} title={group.month}>
            {group.entries.map((entry) => (
              <LedgerRow key={entry.id} entry={entry} counts={stillCounts(entry, now)} />
            ))}
          </Section>
        ))
      )}
    </Screen>
  );
}

function LedgerRow({ entry, counts }: { entry: LedgerEntry; counts: boolean }) {
  const target = ledgerTarget(entry);
  const discipline = disciplineLabel[entry.discipline as Discipline] ?? entry.discipline;
  const label = `${sourceLabel(entry.source)} · ${discipline}`;
  return (
    <Card
      accessibilityLabel={`${label}, ${entry.points} points, ${formatDate(entry.occurred_at)}`}
      onPress={target ? () => router.push(target) : undefined}
    >
      <View style={{ flexDirection: "row", alignItems: "center", gap: space.md }}>
        <View style={{ flex: 1, gap: space.xxs }}>
          <Text variant="label" tone={counts ? "text" : "textMuted"}>
            {label}
          </Text>
          <Text variant="caption" tone="textMuted">
            {formatDate(entry.occurred_at)}
            {counts ? "" : " · no longer counts"}
          </Text>
        </View>
        <Text variant="subheading" tone={counts && entry.points > 0 ? "primaryText" : "textMuted"}>
          {pointsLabel(entry.points)}
        </Text>
      </View>
    </Card>
  );
}
