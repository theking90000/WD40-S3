const numbers = new Intl.NumberFormat("fr-BE");
const decimals = new Intl.NumberFormat("fr-BE", { maximumFractionDigits: 2 });
const percentages = new Intl.NumberFormat("fr-BE", { style: "percent", maximumFractionDigits: 1 });
const units = ["o", "Ko", "Mo", "Go", "To", "Po", "Eo"];

export function formatCount(value: number) {
  return numbers.format(value);
}

export function formatBytes(value: number) {
  let unit = 0;
  while (value >= 1000 && unit < units.length - 1) {
    value /= 1000;
    unit += 1;
  }
  return `${decimals.format(value)} ${units[unit]}`;
}

export function formatPercentage(ratio: number) {
  return percentages.format(ratio);
}

