create table person(
  first_name text not null,
  last_name text not null
);

insert into person(first_name,last_name)
values
  ('Ada','Lovelace'),
  ('Grace','Hopper'),
  ('Katherine','Johnson');
