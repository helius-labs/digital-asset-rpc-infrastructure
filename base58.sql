-- Run this manually onto your DB. You can use it to convert binary to base58.
CREATE OR REPLACE FUNCTION base58(input_bytea bytea)
RETURNS text AS $body$
declare
  alphabet text[] = array[
    '1','2','3','4','5','6','7','8','9', 
    'A','B','C','D','E','F','G','H','J','K','L','M','N','P','Q','R','S','T','U','V','W','X','Y','Z',
    'a','b','c','d','e','f','g','h','i','j','k','m','n','o','p','q','r','s','t','u','v','w','x','y','z'
  ];
  cnt integer = 58;
  dst text = '';
  _mod numeric;
  num numeric = 0;
begin
    -- Convert bytea to numeric
    FOR i IN 1..octet_length(input_bytea) LOOP
        num := num * 256 + get_byte(input_bytea, i - 1);
    END LOOP;

    -- Convert numeric to base58
    while (num >= cnt) loop
        _mod = num % cnt;
        num = (num - _mod) / cnt;
        dst = alphabet[_mod+1] || dst;
    end loop;
    return alphabet[num+1] || dst;
end;
$body$

LANGUAGE 'plpgsql'
IMMUTABLE
CALLED ON NULL INPUT
COST 100;