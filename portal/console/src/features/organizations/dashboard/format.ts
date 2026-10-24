const numbers = new Intl.NumberFormat("fr-BE");
const decimals = new Intl.NumberFormat("fr-BE", { maximumFractionDigits: 2 });
const percentages = new Intl.NumberFormat("fr-BE", { style: "percent", maximumFractionDigits: 1 });
const units = ["o", "Ko", "Mo", "Go", "To", "Po", "Eo"];
const chartDates = new Intl.DateTimeFormat("fr-BE", { day: "numeric", month: "short", timeZone: "UTC" });
const uploadDates = new Intl.DateTimeFormat("fr-BE", { dateStyle: "medium", timeZone: "UTC" });

export function formatCount(value: number) {
  return numbers.format(value);
}

export function formatBytes(value: number, reference = value) {
  let unit = 0;
  while (reference >= 1000 && unit < units.length - 1) {
    reference /= 1000;
    value /= 1000;
    unit += 1;
  }
  return `${decimals.format(value)} ${units[unit]}`;
}

export function formatPercentage(ratio: number) {
  return percentages.format(ratio);
}

export function formatChartDate(timestamp: number) {
  return chartDates.format(new Date(timestamp * 1000));
}

export function formatUploadDate(timestamp: number) {
  return uploadDates.format(new Date(timestamp * 1000));
}

