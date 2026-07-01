-- Sequence table for globally-unique, ordered account numbers.
-- AUTO_INCREMENT guarantees no race conditions under concurrent inserts.
CREATE TABLE account_number_seq (
    id BIGINT UNSIGNED NOT NULL AUTO_INCREMENT,
    PRIMARY KEY (id)
) AUTO_INCREMENT = 1;

-- Config table for supported currencies. Seeded here; adding a new currency
-- requires both a migration row and a matching `CurrencyCode` enum variant.
CREATE TABLE supported_currencies (
    currency VARCHAR(3) NOT NULL,
    PRIMARY KEY (currency)
);

INSERT INTO supported_currencies (currency) VALUES ('NGN'), ('GBP'), ('USD'), ('KES');
