// Decimal byte units and UTC chart boundaries match the metrics API.
export function createDashboardFormatters(locale: string) {
  const numbers = new Intl.NumberFormat(locale);
  const decimals = new Intl.NumberFormat(locale, { maximumFractionDigits: 2 });
  const percentages = new Intl.NumberFormat(locale, { style: "percent", maximumFractionDigits: 1 });
  const units = locale.startsWith("fr")
    ? ["o", "Ko", "Mo", "Go", "To", "Po", "Eo"]
    : ["B", "kB", "MB", "GB", "TB", "PB", "EB"];
  const chartDates = new Intl.DateTimeFormat(locale, { day: "numeric", month: "short", timeZone: "UTC" });
  const uploadDates = new Intl.DateTimeFormat(locale, { dateStyle: "medium", timeZone: "UTC" });
  const chartTimes = new Intl.DateTimeFormat(locale, { hour: "2-digit", minute: "2-digit", hourCycle: "h23", timeZone: "UTC" });
  const uploadDateTimes = new Intl.DateTimeFormat(locale, { dateStyle: "medium", timeStyle: "short", hourCycle: "h23", timeZone: "UTC" });

  function formatCount(value: number) {
    return numbers.format(value);
  }

  function formatBytes(value: number, reference = value) {
    let unit = 0;
    while (reference >= 1000 && unit < units.length - 1) {
      reference /= 1000;
      value /= 1000;
      unit += 1;
    }
    return `${decimals.format(value)} ${units[unit]}`;
  }

  function formatPercentage(ratio: number) {
    return percentages.format(ratio);
  }

  function formatChartDate(timestamp: number, resolution = 86_400) {
    return (resolution < 86_400 ? chartTimes : chartDates).format(new Date(timestamp * 1000));
  }

  function formatUploadDate(timestamp: number, resolution = 86_400) {
    return (resolution < 86_400 ? uploadDateTimes : uploadDates).format(new Date(timestamp * 1000));
  }

  return { formatCount, formatBytes, formatPercentage, formatChartDate, formatUploadDate };
}
