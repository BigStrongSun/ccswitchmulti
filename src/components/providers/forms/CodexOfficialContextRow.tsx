import { useRef } from "react";
import { useTranslation } from "react-i18next";
import { Input } from "@/components/ui/input";
import type { FetchedModel } from "@/lib/api/model-fetch";

interface Props {
  model: string;
  value: string;
  discovered?: FetchedModel;
  onChange: (value: string) => void;
}

export function CodexOfficialContextRow({
  model,
  value,
  discovered,
  onChange,
}: Props) {
  const { t } = useTranslation();
  const inputRef = useRef<HTMLInputElement>(null);
  const defaultWindow = discovered?.contextWindow;
  const maximum = discovered?.maxContextWindow;
  // An explicit default is safe to restore/lower, but is not evidence of a larger maximum.
  const ceiling = maximum ?? defaultWindow;
  const presets = [128_000, 256_000, 512_000, 1_000_000].filter(
    (tokens) =>
      ceiling &&
      tokens <= ceiling &&
      tokens !== defaultWindow &&
      tokens !== maximum,
  );
  const selected = !value
    ? "default"
    : maximum && Number(value) === maximum
      ? String(maximum)
      : Number(value) === defaultWindow || presets.includes(Number(value))
        ? String(Number(value))
        : "custom";
  const invalid = Boolean(
    value && (Number(value) <= 0 || (ceiling && Number(value) > ceiling)),
  );
  const unknown = t("codexConfig.officialTokensUnknown");
  const metrics = [
    [
      t("codexConfig.officialContextDefault"),
      defaultWindow?.toLocaleString() ?? unknown,
    ],
    [t("codexConfig.officialContextMax"), maximum?.toLocaleString() ?? unknown],
    [
      t("codexConfig.officialMaxInput"),
      discovered?.maxInputTokens?.toLocaleString() ?? unknown,
    ],
    [
      t("codexConfig.officialMaxOutput"),
      discovered?.maxOutputTokens?.toLocaleString() ?? unknown,
    ],
  ];

  return (
    <div
      className="min-w-0 space-y-2 border-b border-border-default pb-3"
      role="group"
      aria-label={model}
    >
      <div className="break-all text-sm font-medium">{model}</div>
      <dl className="grid grid-cols-2 gap-x-4 gap-y-2 text-xs md:grid-cols-4">
        {metrics.map(([label, text]) => (
          <div key={label} className="min-w-0">
            <dt className="text-muted-foreground">{label}</dt>
            <dd className="break-words tabular-nums">{text}</dd>
          </div>
        ))}
      </dl>
      <p className="text-xs text-muted-foreground">
        {maximum
          ? maximum > (defaultWindow ?? 0)
            ? t("codexConfig.officialContextCanRaise", {
                max: maximum.toLocaleString(),
              })
            : t("codexConfig.officialContextCanLower")
          : t("codexConfig.officialContextUnverified")}
      </p>
      <div className="grid min-w-0 grid-cols-1 gap-2 sm:grid-cols-2">
        <label className="min-w-0 space-y-1 text-xs">
          <span>{t("codexConfig.officialContextPreset")}</span>
          <select
            className="h-9 w-full min-w-0 rounded-md border border-input bg-background px-2 text-sm"
            aria-label={`${model} ${t("codexConfig.officialContextPreset")}`}
            value={selected}
            onChange={(event) => {
              if (event.target.value === "custom") {
                inputRef.current?.focus();
              } else {
                onChange(
                  event.target.value === "default" ? "" : event.target.value,
                );
              }
            }}
          >
            <option value="default">
              {t("codexConfig.officialContextFollowDefault")}
            </option>
            {presets.map((tokens) => (
              <option key={tokens} value={tokens}>
                {tokens.toLocaleString()}
              </option>
            ))}
            {defaultWindow && defaultWindow !== maximum && (
              <option value={defaultWindow}>
                {t("codexConfig.officialContextDefault")} ·{" "}
                {defaultWindow.toLocaleString()}
              </option>
            )}
            {maximum && (
              <option value={maximum}>
                {t("codexConfig.officialContextUseMax", {
                  max: maximum.toLocaleString(),
                })}
              </option>
            )}
            <option value="custom">
              {t("codexConfig.officialContextCustom")}
            </option>
          </select>
        </label>
        <label className="min-w-0 space-y-1 text-xs">
          <span>{t("codexConfig.officialContextWindow")} (tokens)</span>
          <Input
            ref={inputRef}
            aria-label={`${model} ${t("codexConfig.officialContextWindow")}`}
            aria-invalid={invalid}
            inputMode="numeric"
            disabled={!ceiling}
            value={value}
            placeholder={defaultWindow?.toLocaleString() ?? unknown}
            onChange={(event) =>
              onChange(event.target.value.replace(/[^0-9]/g, ""))
            }
          />
        </label>
      </div>
      {invalid && (
        <p className="text-xs text-destructive" role="alert">
          {t("codexConfig.officialContextRange", {
            max: ceiling?.toLocaleString() ?? unknown,
          })}
        </p>
      )}
    </div>
  );
}
