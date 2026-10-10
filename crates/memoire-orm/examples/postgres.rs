//! PostgreSQL with a connection pool.
//!
//! ```bash
//! DATABASE_URL=postgres://postgres:postgres@localhost/postgres \
//!     cargo run -p memoire-orm --example postgres --features postgres
//! ```

use memoire_orm::prelude::*;

#[derive(Model, Debug, Clone)]
#[memoire(table = "example_products")]
pub struct Product {
    #[memoire(primary_key, auto_increment)]
    pub id: i64,
    #[memoire(unique)]
    pub sku: String,
    pub name: String,
    pub price_cents: i64,
    pub stock: i32,
}

#[tokio::main]
async fn main() -> Result<()> {
    let url =
        std::env::var("DATABASE_URL").unwrap_or_else(|_| "postgres://postgres:postgres@localhost/postgres".into());
    let db = PgPool::connect(&url).await?;
    db.drop_table_if_exists::<Product>().await?;
    db.create_table::<Product>().await?;

    let products: Vec<Product> = (1..=1000)
        .map(|i| Product {
            id: 0,
            sku: format!("SKU-{i:05}"),
            name: format!("Product {i}"),
            price_cents: 199 + i * 10,
            stock: (i % 50) as i32,
        })
        .collect();
    let inserted = Product::insert_many(&db, &products).await?;
    println!("inserted {inserted} products in one round trip");

    // Concurrent queries share the pool.
    let (cheap, out_of_stock, page) = tokio::try_join!(
        Product::query().filter(Product::PRICE_CENTS.lt(1_000)).count(&db),
        Product::query().filter(Product::STOCK.eq(0)).count(&db),
        Product::query().order_by(Product::PRICE_CENTS.desc()).paginate(&db, 1, 5),
    )?;
    println!("{cheap} cheap products, {out_of_stock} out of stock");
    for p in page.items {
        println!("{} {} {:.2} €", p.sku, p.name, p.price_cents as f64 / 100.0);
    }

    let mut first = Product::get(&db, 1).await?;
    first.stock += 10;
    first.update(&db).await?;
    println!("restocked {} → {}", first.sku, first.stock);

    db.drop_table::<Product>().await?;
    Ok(())
}
