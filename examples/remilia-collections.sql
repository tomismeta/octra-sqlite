create table collection(
  name text primary key,
  opensea_slug text not null,
  chain text not null,
  relationship text not null,
  launched_month text not null,
  date_precision text not null
);

insert into collection(
  name,
  opensea_slug,
  chain,
  relationship,
  launched_month,
  date_precision
)
values
  ('Milady Maker','milady','Ethereum','Remilia','2021-08-01','month'),
  ('Banners NFT','banners-nft','Ethereum','Remilia','2022-07-01','month'),
  ('Redacted Remilio Babies','remilio-babies','Ethereum','Remilia','2022-08-01','month'),
  ('SchizoPosters','schizoposters','Ethereum','Remilia adjacent','2023-03-01','month'),
  ('Bonkler','bonkler','Ethereum','Remilia','2023-04-01','month'),
  ('YAYO NFT','yayo-nft','Ethereum','Remilia adjacent','2023-05-01','month'),
  ('Milady Fumo Babies','miladyfumo','Ethereum','Remilia adjacent','2023-12-01','month'),
  ('Yumemono','yumemono','Ethereum','Remilia adjacent','2025-03-01','month'),
  ('World Computer Netizens','world-computer-netizens-megaeth','MegaETH','Remilia adjacent','2026-02-01','month'),
  ('moemoe LLC','moemoe-llc','Ethereum','Remilia adjacent','2026-02-01','month');
