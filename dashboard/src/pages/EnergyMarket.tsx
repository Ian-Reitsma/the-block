import React from "react";

type MetricCardProps = {
  title: string;
  value: string | number;
  unit?: string;
};

const MetricCard: React.FC<MetricCardProps> = ({ title, value, unit }) => (
  <div className="metric-card">
    <div className="metric-title">{title}</div>
    <div className="metric-value">
      {value}
      {unit ? <span className="metric-unit"> {unit}</span> : null}
    </div>
  </div>
);

type ProviderSummary = {
  providerId: string;
  capacityKwh: number;
  reputation: number;
  jurisdiction: string;
};

type Settlement = {
  buyer: string;
  seller: string;
  kwh: number;
  price: number;
  block: number;
};

type EnergyMarketProps = {
  totalKwh: number;
  providerCount: number;
  energyProviders: ProviderSummary[];
  recentSettlements: Settlement[];
};

export function EnergyMarket({
  totalKwh,
  providerCount,
  energyProviders,
  recentSettlements,
}: EnergyMarketProps) {
  return (
    <div className="energy-market">
      <h1>World OS – Energy Market</h1>
      <div className="metric-grid">
        <MetricCard title="Total Energy Traded" value={totalKwh} unit="kWh" />
        <MetricCard title="Active Providers" value={providerCount} />
      </div>
      <section>
        <h2>Providers</h2>
        <table>
          <thead>
            <tr>
              <th>Provider</th>
              <th>Capacity (kWh)</th>
              <th>Reputation</th>
              <th>Jurisdiction</th>
            </tr>
          </thead>
          <tbody>
            {energyProviders.map((provider) => (
              <tr key={provider.providerId}>
                <td>{provider.providerId}</td>
                <td>{provider.capacityKwh.toLocaleString()}</td>
                <td>{provider.reputation.toFixed(2)}</td>
                <td>{provider.jurisdiction}</td>
              </tr>
            ))}
          </tbody>
        </table>
      </section>
      <section>
        <h2>Recent Settlements</h2>
        <table>
          <thead>
            <tr>
              <th>Block</th>
              <th>Buyer</th>
              <th>Seller</th>
              <th>kWh</th>
              <th>Price (BLOCK)</th>
            </tr>
          </thead>
          <tbody>
            {recentSettlements.map((settlement, idx) => (
              <tr key={`${settlement.block}-${idx}`}>
                <td>{settlement.block}</td>
                <td>{settlement.buyer}</td>
                <td>{settlement.seller}</td>
                <td>{settlement.kwh}</td>
                <td>{settlement.price}</td>
              </tr>
            ))}
          </tbody>
        </table>
      </section>
    </div>
  );
}
